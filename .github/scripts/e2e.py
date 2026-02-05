import os
import subprocess
import re
import json


INIT_FOLDER = "init"
CONFIG_FILE = "rproject.toml"
RV_REPO_1 = "{ alias = 'repo1', url = 'https://a2-ai.github.io/rv-test-repo/repo1'}"
RV_REPO_2 = "{ alias = 'repo2', url = 'https://a2-ai.github.io/rv-test-repo/repo2'}"

def run_cmd(cmd = [str]):
    result = subprocess.run(cmd, capture_output=True, text=True)
    print(result.stderr)
    print(result.stdout)
    if result.returncode != 0:
        print(f"Command failed with error: {result.stderr}")
        exit(1)
    
    return result.stdout

def run_rv_cmd(cmd = str, args = [str]):
    print(f">> Running rv {cmd}")
    command = ["rv", cmd, "-vvv"] + args
    return run_cmd(command)
    

def run_r_script(script = str):
    print(f">> Running R script: {script}")
    command = ["Rscript", "-e", script]
    return run_cmd(command)

def edit_r_version(file_path: str, r_version: str) -> None:
    with open(file_path, "r") as f:
        content = f.read()

    # Replace the first r_version value anywhere in the file
    updated = re.sub(
        r'(\br_version\s*=\s*")[^"]+(")',
        rf'\g<1>{r_version}\g<2>',
        content,
        count=1,
    )

    with open(file_path, "w") as f:
        f.write(updated)
        
def check_r_profile(r_versions_match = bool):
    if r_versions_match:
        expected_lib_elem = f"{INIT_FOLDER}/rv/library"
    else:
        expected_lib_elem = "__rv_R_mismatch"
        
    if expected_lib_elem not in run_r_script(".libPaths()"):
        print(f".libPaths not set correctly upon init")
        exit(1)

    if "rv-test-repo/repo2" not in run_r_script("getOption('repos')"):
        print(f"repos not set correctly upon init")
        exit(1)

def run_test():
    os.environ["PATH"] = f"{os.path.abspath('./target/release')}:{os.environ.get('PATH', '')}"
    run_rv_cmd("init", [INIT_FOLDER, "--no-repositories", "--force"])
    original_dir = os.getcwd()
    os.chdir(INIT_FOLDER)
    
    
    try: 
        run_rv_cmd("configure", ["repository", "add", "repo2", "--url", "https://a2-ai.github.io/rv-test-repo/repo2"])
        check_r_profile(True)
        run_r_script(".rv$add('rv.git.pkgA', dry_run=TRUE)")
        summary = run_rv_cmd("summary", [])
        if "Installed: 0/0" not in summary:
            print("rv add --dry-run effected the config")
            
        run_rv_cmd("add", ["rv.git.pkgA", "--no-sync"])
        summary = run_rv_cmd("summary", [])
        if "Installed: 0/1" not in summary:
            print(f"rv add --no-sync did not behave as expected")
            
        run_rv_cmd("add", ["rv.git.pkgA"])
        run_rv_cmd("configure", ["repository", "add", "repo1", "--url", "https://a2-ai.github.io/rv-test-repo/repo1", "--first"])
        res = run_rv_cmd("sync", [])
        if "Nothing to do" not in res:
            print("Adding repo caused re-sync")
            exit(1)
        res = run_rv_cmd("upgrade", [])
        if "- rv.git.pkgA" not in res or "+ rv.git.pkgA" not in res or "0.0.5" not in res or "https://a2-ai.github.io/rv-test-repo/repo1" not in res:
            print("Upgrade did not behave as expected")
            exit(1)
            
        res = run_rv_cmd("cache", ["--json"])
        cache_data = json.loads(res)
        
        for repo in cache_data.get("repositories", []):
            if not repo["path"].endswith(repo["hash"]):
                print(f"Path {repo['path']} does not end with hash {repo['hash']}")
                exit(1)        
         
        edit_r_version(CONFIG_FILE, "4.3")
        check_r_profile(False)

    finally:
        os.chdir(original_dir)

if __name__ == "__main__":
    exit(run_test())