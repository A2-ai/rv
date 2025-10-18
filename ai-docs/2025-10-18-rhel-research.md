# RHEL/RPM System Dependencies Support - Research & Implementation Plan

## Research Summary

### Posit Package Manager API Distribution Mapping

**API Endpoint Compatibility Matrix:**

| Distribution | Version 8 | Version 9 | Notes |
|-------------|-----------|-----------|-------|
| CentOS | ✅ Works | ❌ Unsupported | Use centos8 for EL8 builds |
| RedHat | ✅ Works | ✅ Works | Uses `subscription-manager` in pre_install |
| RockyLinux | N/A | ✅ Works | Uses `crb` repo instead of `powertools` |
| AlmaLinux | Use centos8 | Use rockylinux9 | Not directly supported by API |

**Key API Response Differences:**

1. **CentOS 8** - Pre-install hooks:
   ```json
   "pre_install": [
     {"command": "dnf install -y dnf-plugins-core"},
     {"command": "dnf config-manager --set-enabled powertools"},
     {"command": "dnf install -y epel-release"}
   ]
   ```

2. **RedHat 8** - Uses subscription-manager:
   ```json
   "pre_install": [
     {"command": "subscription-manager repos --enable codeready-builder-for-rhel-8-$(arch)-rpms"},
     {"command": "dnf install -y https://dl.fedoraproject.org/pub/epel/epel-release-latest-8.noarch.rpm"}
   ]
   ```

3. **RockyLinux 9** - Uses `crb` instead of `powertools`:
   ```json
   "pre_install": [
     {"command": "dnf install -y dnf-plugins-core"},
     {"command": "dnf config-manager --set-enabled crb"},
     {"command": "dnf install -y epel-release"}
   ]
   ```

### RPM Package Query Commands

**Testing Results:**
- `rpm -q package1 package2 ...` - Query multiple packages
- Exit code: 0 = all installed, 1 = any not installed
- Installed packages print to stdout: `packagename-version-release.arch`
- Non-installed packages print to stderr: `package X is not installed`
- Filter installed: `rpm -q ... 2>&1 | grep -v "is not installed"`
- Extract names: Parse stdout, split on first hyphen before version

**Comparison with dpkg:**
- dpkg: `dpkg-query -W -f='${Package}\n' packages...`
- rpm: `rpm -q --queryformat '%{NAME}\n' packages...` (but errors still on stderr)
- Both allow batch queries, making them efficient

### Current rv Implementation Status

**Files:**
- `src/system_info.rs` - OS detection via os_info crate (v3.12.0)
- `src/system_req.rs` - System requirements checking
  - Line 148-179: dpkg implementation (Ubuntu/Debian)
  - Line 181-189: Placeholder for other distributions
  - Line 86-91: Already lists supported RPM distributions in `is_supported()`

**Missing:**
- AlmaLinux case in `system_info.rs` (Type::AlmaLinux exists in os_info 3.12.0)
- RPM implementation in `check_installation_status()`
- Distribution mapping logic for API calls (AlmaLinux → CentOS 8 or RockyLinux 9)

---

## Implementation Plan

### Phase 1: Manual Testing & Validation ✅ COMPLETED

**Goal:** Verify the approach works outside of rv before integrating

**Tasks:**

1. **Test API endpoint calls manually** ✅
   - Verified centos8, redhat8/9, rockylinux9 responses
   - Documented package naming patterns

2. **Test rpm query commands manually** ✅
   - Tested with packages from API responses (e.g., `libcurl-devel`, `openssl-devel`)
   - Verified parsing logic:
     ```bash
     rpm -q libcurl-devel openssl-devel fake-pkg 2>&1
     rpm -q bash fake1 fake2 2>&1 | grep -v "is not installed"
     ```

3. **Test with sf package dependencies** ✅
   ```bash
   # Query all deps for sf package on centos8
   curl -s 'https://packagemanager.posit.co/__api__/repos/cran/packages/sf/sysreqs?distribution=centos&release=8'
   # Test: rpm -q gdal-devel gdal sqlite-devel geos-devel proj-devel
   ```

### Phase 2: Add Distribution Mapping Logic

**Goal:** Map AlmaLinux and other RHEL-like distros to correct API endpoints

**File:** `src/system_info.rs`

**Changes:**

1. Add AlmaLinux case (line ~87):
   ```rust
   Type::AlmaLinux => OsType::Linux("almalinux"),
   ```

2. Add API distribution mapping helper in `impl SystemInfo`:
   ```rust
   /// Returns the distribution name to use for Posit Package Manager API
   /// Some distros need to be mapped to compatible API endpoints
   pub fn api_distribution(&self) -> &'static str {
       match self.os_type {
           OsType::Linux(distrib) => match distrib {
               "almalinux" => {
                   // AlmaLinux 8 -> centos, AlmaLinux 9 -> rockylinux
                   match self.version {
                       Version::Semantic(major, _, _) if major < 9 => "centos",
                       _ => "rockylinux",
                   }
               },
               // CentOS 9 is unsupported, map to rockylinux
               "centos" => {
                   match self.version {
                       Version::Semantic(major, _, _) if major >= 9 => "rockylinux",
                       _ => "centos",
                   }
               },
               // For Oracle Linux, use redhat
               "oracle" => "redhat",
               // Everything else maps to itself
               _ => distrib,
           },
           _ => "invalid",
       }
   }
   ```

3. Update `sysreq_data()` to use `api_distribution()` (line 117):
   ```rust
   pub fn sysreq_data(&self) -> (&'static str, String) {
       match self.os_type {
           OsType::Linux(distrib) => {
               let api_distrib = self.api_distribution();
               match distrib {
                   "suse" => ("sle", self.version.to_string()),
                   "ubuntu" => {
                       let version = match self.version {
                           Version::Semantic(year, month, _) => {
                               format!("{year}.{}{month}", if month < 10 { "0" } else { "" })
                           }
                           _ => unreachable!(),
                       };
                       (api_distrib, version)
                   }
                   "debian" => match self.version {
                       Version::Semantic(major, _, _) => (api_distrib, major.to_string()),
                       _ => unreachable!(),
                   },
                   _ => (api_distrib, self.version.to_string()),
               }
           },
           _ => ("invalid", String::new()),
       }
   }
   ```

4. Update `is_supported()` to include AlmaLinux (line 80-94):
   ```rust
   pub fn is_supported(system_info: &SystemInfo) -> bool {
       let (distrib, version) = system_info.sysreq_data();

       match distrib {
           "ubuntu" => ["20.04", "22.04", "24.04"].contains(&version.as_str()),
           "debian" => version.starts_with("12"),
           "centos" => version.starts_with("7") || version.starts_with("8"),
           "almalinux" => version.starts_with("8") || version.starts_with("9"),  // NEW
           "redhat" => {
               version.starts_with("7") || version.starts_with("8") || version.starts_with("9")
           }
           "rockylinux" => version.starts_with("8") || version.starts_with("9"),
           "opensuse" | "sle" => version.starts_with("15"),
           _ => false,
       }
   }
   ```

### Phase 3: Implement RPM Package Checking

**Goal:** Add RPM-based package installation detection

**File:** `src/system_req.rs`

**Changes:**

1. **Add RPM checking logic** (replace lines 181-189):
   ```rust
   "centos" | "almalinux" | "redhat" | "rockylinux" | "fedora" | "opensuse" | "sle" => {
       // Running rpm -q {..pkg_list} and parse stdout
       let command = Command::new("rpm")
           .arg("-q")
           .args(sys_deps)
           .output()
           .expect("to be able to run rpm command");

       let stdout = String::from_utf8(command.stdout).unwrap();
       let stderr = String::from_utf8(command.stderr).unwrap();

       // Parse stdout for installed packages
       // Format: "packagename-version-release.arch"
       for line in stdout.lines() {
           let line = line.trim();
           if !line.is_empty() {
               // Extract package name (everything before first hyphen followed by a digit)
               if let Some(pkg_name) = extract_rpm_package_name(line) {
                   if let Some(status) = out.get_mut(pkg_name) {
                       *status = SysInstallationStatus::Present;
                   }
               }
           }
       }

       // Also check stderr to see if any packages printed "not installed" messages
       // This helps us mark things as definitively Absent vs Unknown
       for line in stderr.lines() {
           // Format: "package NAME is not installed"
           if line.contains("is not installed") {
               if let Some(pkg_name) = line.split_whitespace().nth(1) {
                   if let Some(status) = out.get_mut(pkg_name) {
                       if status == &SysInstallationStatus::Unknown {
                           *status = SysInstallationStatus::Absent;
                       }
                   }
               }
           }
       }

       // Check PATH for known tools (same as dpkg logic)
       let mut to_check_in_path: Vec<_> = from_env.split(",").map(|x| x.trim()).collect();
       to_check_in_path.extend_from_slice(KNOWN_THINGS_IN_PATH);

       for (name, status) in out
           .iter_mut()
           .filter(|(_, v)| v == &&SysInstallationStatus::Unknown)
       {
           if to_check_in_path.contains(&name.as_str()) {
               if which(name).is_ok() {
                   *status = SysInstallationStatus::Present;
               } else {
                   *status = SysInstallationStatus::Absent;
               }
           }
       }
   }
   ```

2. **Add helper function** (before `check_installation_status`):
   ```rust
   /// Extract package name from rpm query output
   /// Input: "bash-4.4.20-6.el8_10.x86_64"
   /// Output: Some("bash")
   ///
   /// RPM package naming: name-version-release.arch
   /// We need to split on the first hyphen that's followed by a version number
   fn extract_rpm_package_name(rpm_output: &str) -> Option<&str> {
       // Find the first hyphen followed by a digit (start of version)
       let mut last_name_idx = 0;
       let chars: Vec<char> = rpm_output.chars().collect();

       for i in 0..chars.len().saturating_sub(1) {
           if chars[i] == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
               last_name_idx = i;
               break;
           }
       }

       if last_name_idx > 0 {
           Some(&rpm_output[..last_name_idx])
       } else {
           // Fallback: if no version found, might be just a package name
           if !rpm_output.contains('-') {
               Some(rpm_output)
           } else {
               None
           }
       }
   }
   ```

### Phase 4: Testing & Validation

**Goal:** Ensure the implementation works correctly on Alma8

**Tasks:**

1. **Create test cases** in `src/system_req.rs`:
   ```rust
   #[cfg(test)]
   mod test {
       use super::*;

       #[test]
       fn test_extract_rpm_package_name() {
           assert_eq!(extract_rpm_package_name("bash-4.4.20-6.el8_10.x86_64"), Some("bash"));
           assert_eq!(extract_rpm_package_name("libcurl-devel-7.61.1-34.el8_10.8.x86_64"), Some("libcurl-devel"));
           assert_eq!(extract_rpm_package_name("abseil-cpp-devel-20210324.2-1.el8.x86_64"), Some("abseil-cpp-devel"));
           assert_eq!(extract_rpm_package_name("bash"), Some("bash"));
       }

       #[test]
       fn test_alma8_is_supported() {
           let system = SystemInfo::new(
               OsType::Linux("almalinux"),
               Some("x86_64".to_string()),
               None,
               "8.10"
           );
           assert!(is_supported(&system));
       }

       #[test]
       fn test_alma8_api_mapping() {
           let system = SystemInfo::new(
               OsType::Linux("almalinux"),
               Some("x86_64".to_string()),
               None,
               "8.10"
           );
           let (distrib, version) = system.sysreq_data();
           assert_eq!(distrib, "centos");
           assert_eq!(version, "8.10");
       }

       #[test]
       fn test_alma9_api_mapping() {
           let system = SystemInfo::new(
               OsType::Linux("almalinux"),
               Some("x86_64".to_string()),
               None,
               "9.0"
           );
           let (distrib, version) = system.sysreq_data();
           assert_eq!(distrib, "rockylinux");
           assert_eq!(version, "9.0");
       }
   }
   ```

2. **Run unit tests:**
   ```bash
   cd /home/admin/repos/a2-ai/rv
   cargo test --features=cli extract_rpm_package_name
   cargo test --features=cli alma
   ```

3. **Integration test on Alma8:**
   ```bash
   # Create a test project with packages that have system deps
   cd /tmp/rv-test-rpm
   cat > rproject.toml << 'EOF'
   [project]
   name = "rpm-test"
   r_version = "4.4"
   repositories = [
       {alias = "posit", url = "https://packagemanager.posit.co/cran/latest/"}
   ]
   dependencies = [
       "sf",      # Has many system deps
       "curl",    # Has libcurl-devel, openssl-devel
       "units",   # Has udunits2-devel
   ]
   EOF

   # Initialize and check system deps
   rv init
   rv sysdeps --json
   rv sysdeps  # Human readable output
   ```

4. **Verify output shows:**
   - Package names correctly detected
   - Installation status (present/absent) accurate
   - No errors/panics

### Phase 5: Support for Future Distributions

**Goal:** Make it easy to add support for EL9, other RPM distros

**Documentation to add in code comments:**

```rust
// Distribution mapping strategy:
// - AlmaLinux 8 -> API: centos8 (most compatible for EL8)
// - AlmaLinux 9 -> API: rockylinux9 (centos9 unsupported)
// - CentOS 8 -> API: centos8
// - CentOS 9 -> API: rockylinux9 (centos9 returns error)
// - RockyLinux 8/9 -> API: rockylinux
// - RedHat 8/9 -> API: redhat (uses subscription-manager)
// - Oracle Linux -> API: redhat (binary compatible)
//
// Note: All EL8-compatible distros can use centos8 API endpoint
//       All EL9-compatible distros should use rockylinux9 or redhat9
```

---

## Implementation Order Summary

1. ✅ **Research & Testing** (Completed)
   - API endpoints tested
   - rpm commands tested
   - Distribution differences documented

2. **Phase 2: Distribution Mapping** (Priority 1 - Alma8 support)
   - Add AlmaLinux to os_info match
   - Add `api_distribution()` method
   - Update `sysreq_data()` to use mapping
   - Update `is_supported()`

3. **Phase 3: RPM Implementation** (Priority 1 - Alma8 support)
   - Add RPM checking logic in `check_installation_status()`
   - Add `extract_rpm_package_name()` helper
   - Handle rpm command output parsing

4. **Phase 4: Testing** (Priority 1 - Alma8 support)
   - Unit tests for helpers
   - Unit tests for distribution mapping
   - Integration test on real Alma8 system

5. **Phase 5: Documentation & Future Support** (Priority 2)
   - Document distribution mapping strategy
   - Prepare for EL9 support (already covered by mapping logic)

---

## Key Design Decisions

1. **Distribution Mapping Approach:**
   - Use `api_distribution()` method to centralize mapping logic
   - AlmaLinux 8 → centos8 (most tested, stable)
   - AlmaLinux 9 → rockylinux9 (centos9 unsupported by API)
   - Makes it easy to adjust mappings if API support changes

2. **RPM Package Name Parsing:**
   - Use heuristic: find first hyphen before a digit (version start)
   - Handles packages with hyphens in names (e.g., `libcurl-devel`, `abseil-cpp-devel`)
   - Robust fallback for edge cases

3. **Error Handling:**
   - Parse both stdout (installed packages) and stderr (error messages)
   - Unknown packages remain "Unknown" unless definitively absent
   - Consistent with existing dpkg implementation

4. **Code Reuse:**
   - Same PATH-checking logic as dpkg
   - Same environment variable support
   - Minimal changes to existing structure

---

## Testing Checklist for Alma8

- [ ] `rv sysdeps` works without errors on Alma8
- [ ] Packages with system deps (sf, curl, units) are detected
- [ ] Installation status accurately reflects rpm -q output
- [ ] Packages installed via dnf show as "present"
- [ ] Packages not installed show as "absent"
- [ ] Unknown packages (not system deps) don't cause errors
- [ ] `--only-absent` flag works correctly
- [ ] `--json` output is valid JSON
- [ ] Unit tests pass
- [ ] No regressions on Ubuntu/Debian

---

This plan prioritizes Alma8/CentOS8 support first (build machines), while laying the groundwork for EL9 and other RPM-based distributions. The distribution mapping strategy makes it easy to adjust as API support evolves.
