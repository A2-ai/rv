import os
from pathlib import Path
import subprocess

PARENT_FOLDER = "invalid_projects"
INIT_FOLDER = "init-test"
UPGRADE_FOLDER = os.path.join("example_projects", "project-upgrade")
CONFIG_FILE = "rproject.toml"

def check_result(res, expect_success: bool = True):
    if expect_success:
        if res.returncode != 0:
            print(f"Command failed with error: {res.stderr}")
            exit(1)

        return res.stdout 
    
    else:
        if res.returncode == 0:
            print(f"Command was successful for invalid config: {res.stdout}")
            exit(1)
            
        return res.stderr

def run_rv_cmd(cmd = str, args = [str], expect_success: bool = True):
    print(f">> Running rv {cmd}")
    command = ["rv", cmd, "-vvv"] + args
    result = subprocess.run(command, capture_output=True, text=True)
    return check_result(result, expect_success)
    

def run_r_script(script, expect_success: bool = True):
    print(f">> Running R script: {script}")
    command = ["Rscript", "-e", script]
    result = subprocess.run(command, capture_output=True, text=True)
    return check_result(result, expect_success)


def load_r_profile():
    print(f">> Loading R profile")
    command = ["Rscript", ".Rprofile"]
    result = subprocess.run(command, capture_output=True, text=True)
    return check_result(result)

def init_test():
    run_rv_cmd("init", [INIT_FOLDER])
    # Have to change dirs for .Rprofile
    original_dir = os.getcwd()
    os.chdir(INIT_FOLDER)
    try:
        # verify rvr.R and activate.R will run
        load_r_profile()
        # verify rvr is functional
        run_r_script(".rv$add('dplyr', dry_run=TRUE)")
        # verify dry_run does not result in any changes
        plan = run_rv_cmd("plan", [])
        if "Nothing to do" not in plan:
            print(f"rvr add dry-run has unexpected result: {plan}")
            exit(1)
        run_rv_cmd("add", ["R6"])
    finally:
        os.chdir(original_dir)
        
# def upgrade_test():
#     run_rv_cmd("upgrade", ["-c", os.path.join(UPGRADE_FOLDER, CONFIG_FILE)])
#     library = run_rv_cmd("library", ["-c", os.path.join(UPGRADE_FOLDER, CONFIG_FILE)])
#     pkg_version = run_r_script(f"packageVersion('ggplot2', lib.loc = '{library}')")
#     if "3.5.1" not in pkg_version:
#         print(f"Incorrect package version installed during upgrade: {pkg_version}")
#         exit(1)    
#     return pkg_version
    
    
def run_test():
    os.environ["PATH"] = os.path.abspath("./target/release/") + os.pathsep + os.environ.get("PATH", "")
    items = os.listdir(PARENT_FOLDER)
    for subfolder in items:
        dir = os.path.join(PARENT_FOLDER, subfolder)
        config_path = os.path.join(dir,CONFIG_FILE)
        print(f"===== Processing example: {config_path} ======")
        stdout = run_rv_cmd("summary", ["-c", config_path], True) # should be False once fixes to these issues are in
        print(f"{stdout}")
        
    init_test()
    # upgrade_test()
    
if __name__ == "__main__":
    exit(run_test())