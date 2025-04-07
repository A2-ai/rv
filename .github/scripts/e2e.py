import os
import re
from pathlib import Path
import subprocess
import shutil

PROJ_PATH = "e2e_test"

def run_rv_cmd(cmd, args):
    print(f">> Running rv {cmd}")
    command = ["rv", cmd, "-vvv"] + args
    result = subprocess.run(command, capture_output=True, text=True)

    # Check for errors
    if result.returncode != 0:
        print(f"Command failed with error: {result.stderr}")
        exit(1)

    return result.stdout    

def run_r_script(script):
    print(f">> Running R script: {script}")
    command = ["Rscript", "-e", script]
    result = subprocess.run(command, capture_output=True, text=True)
    
    # Check for errors
    if result.returncode != 0:
        print(f"Command failed with error: {result.stderr}")
        exit(1)

    return result.stdout

def load_r_profile():
    print(f">> Loading R profile")
    print(f"Current working directory: {os.getcwd()}")
    command = ["Rscript", ".Rprofile"]
    result = subprocess.run(command, capture_output=True, text=True)

    # Check for errors
    if result.returncode != 0:
        print(f"Command failed with error: {result.stderr}")
        exit(1)

    return result.stdout

def edit_repositories():
    print(f">> Replacing repositories field with 'https://packagemanager.posit.co/cran/2025-04-07'")
    file_path = Path("rproject.toml")
    content = file_path.read_text()
    pattern = r'repositories\s*=\s*\[.*?\]'
    replace = 'repositories = [\n\t{alias = "RPSM", url = "https://packagemanager.posit.co/cran/2025-04-07"},\n]'
    new_content = re.sub(pattern, replace, content, flags=re.DOTALL)
    file_path.write_text(new_content)
    
def run_plan_for_failure():
    # After edited to config to change repositories, verify plan results in error
    command = ["rv", "plan", "-vvv"]
    result = subprocess.run(command, capture_output=True, text=True)
    
    # Check for errors
    if result.returncode == 0:
        print(f"Packages resolved when not intended: {result.stdout}")
        exit(1)
        
    return result.stderr

def run_test():
    os.environ["PATH"] = os.path.abspath("./target/release/") + os.pathsep + os.environ.get("PATH", "")
    if os.path.exists(PROJ_PATH):
        shutil.rmtree(PROJ_PATH)
    # Initialize the project, move into it, and load the R profile
    run_rv_cmd("init", [PROJ_PATH])
    os.chdir(PROJ_PATH)
    load_r_profile()
    
    # Add R6 and verify it loads (and .Rprofile loading sets .libPaths)
    run_rv_cmd("add", ["R6"])
    run_r_script("library(R6)")
    run_rv_cmd("summary", [])
    
    # Dry-run adding dplyr using rvr and verify it doesn't make any changes
    run_r_script(".rv$add('dplyr', dry_run = TRUE)")
    plan = run_rv_cmd("plan", [])
    if plan != "Nothing to do\n": 
        print(f"Dry-run adding dplyr leads to changes planned: {plan}")
        exit(1)
        
    # Add fansi, but do not sync to verify the plan is correct
    run_rv_cmd("add", ["fansi", "--no-sync"])
    plan = run_rv_cmd("plan", [])
    if "+ fansi" not in plan:
        print(f"Dry run add did not result in correct plan: {plan}")
        exit(1)
        
    # Edit the repositories field to test that lockfile is invalid if repo not present in config anymore
    edit_repositories()
    run_plan_for_failure()
        
    return 0

if __name__ == "__main__":
    exit(run_test())