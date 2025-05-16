import os
from pathlib import Path
import subprocess

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

def add_repo(file_path, repo = str):
    with open(file_path, "r") as f:
        lines = f.readlines()

    new_lines = []
    in_repos = False
    repo_inserted = False

    for i, line in enumerate(lines):
        stripped = line.strip()

        if not in_repos and stripped.startswith("repositories") and "[" in stripped:
            in_repos = True
            new_lines.append(line)
            continue

        if in_repos and not repo_inserted:
            if stripped.startswith("]"):
                new_lines.append(f"    {repo},\n")
                new_lines.append(line)
                repo_inserted = True
                in_repos = False
                continue
            else:
                # Insert the new repo before the first existing entry
                new_lines.append(f"    {repo},\n")
                new_lines.append(line)
                repo_inserted = True
                continue

        new_lines.append(line)

    with open(file_path, "w") as f:
        f.writelines(new_lines)
        
def check_r_profile():
    if f"{INIT_FOLDER}/rv/library" not in run_r_script(".libPaths()"):
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
        add_repo(CONFIG_FILE, RV_REPO_2)
        run_r_script("source('.Rprofile')")
        check_r_profile()
        run_r_script(".rv$add('rv.git.pkgA', dry_run=TRUE)")
        summary = run_rv_cmd("summary", [])
        if "Installed: 0/0" not in summary:
            print("rv add --dry-run effected the config")
            
        run_rv_cmd("add", ["rv.git.pkgA", "--no-sync"])
        summary = run_rv_cmd("summary", [])
        if "Installed: 0/1" not in summary:
            print(f"rv add --no-sync did not behave as expected")
            
        run_rv_cmd("add", ["rv.git.pkgA"])
        add_repo(CONFIG_FILE, RV_REPO_1)
        res = run_rv_cmd("sync", [])
        if "Nothing to do" not in res:
            print("Adding repo caused re-sync")
        res = run_rv_cmd("upgrade", [])
        if "- rv.git.pkgA" not in res or "+ rv.git.pkgA (0.0.5" not in res or "from https://a2-ai.github.io/rv-test-repo/repo1)" not in res:
            print("Upgrade did not behave as expected")
    finally:
        os.chdir(original_dir)

if __name__ == "__main__":
    exit(run_test())