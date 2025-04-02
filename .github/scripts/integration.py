import json
import os
import subprocess
import shutil

PARENT_FOLDER = "example_projects"

def run_cmd(cmd, path, json = False):
    additional_args = ["--json"] if json else []
    print(f">> Running rv {cmd}")
    command = ["./target/release/rv", cmd, "--config-file", path, "-vvv"] + additional_args
    result = subprocess.run(command, capture_output=True, text=True)
    if not json:
        print(result.stdout)
        print(result.stderr)

    # Check for errors
    if result.returncode != 0:
        print(f"Command failed with error: {result.stderr}")
        exit(1)

    return result.stdout

def install_tinytest():
    print(">> Installing tinytest")
    result = subprocess.run(["Rscript", "-e", "install.packages('tinytest')"], check=True)
    if result.returncode != 0:
        print(f"Command failed with error: {result.stderr}")
        exit(1)

    return result.stdout

def run_r_test(library_path, test_folder):
    print(">> Running R test")
    library_path = library_path.removesuffix("\n")
    # test_cmd = f"Rscript -e \"lib_loc <- '{library_path}'; res <- tinytest::run_test_dir('{test_folder}', verbose = FALSE); if (isTRUE(as.logical(res))) res else stop(paste0(capture.output(res)), collapse = '\n'))\""
    test_cmd = f"Rscript -e \"lib_loc <- '{library_path}'; res <- tinytest::run_test_dir('{test_folder}', verbose = FALSE); write(capture.output(res), ifelse(isTRUE(as.logical(res)), stdout(), stderr()))\""
    result = subprocess.run(test_cmd, shell=True, capture_output=True, text=True)
    if result.returncode != 0:
        print(f"Command failed with error: {result.stderr}")
        exit(1)
        
    if result.stderr != "":
        print(f"Test failed with result:\n{result.stderr}")
        exit(1)
        
    return result.stdout


def run_examples():
    install_tinytest()
    items = os.listdir(PARENT_FOLDER)
    items = ["archive"]
    for subfolder in items:
        # This one needs lots of system deps, skipping in CI
        if subfolder == "big":
            continue
        subfolder_path = os.path.join(PARENT_FOLDER, subfolder, "rproject.toml")
        print(f"===== Processing example: {subfolder_path} =====")

        # The git packages depend on each other but we don't want
        if "git" in subfolder_path:
            out = run_cmd("cache", subfolder_path, ["--json"])
            if out:
                cache_data = json.loads(out)
                for obj in cache_data.get("git", []):
                    print(f"Clearing cache: {obj}")
                    shutil.rmtree(obj["source_path"], ignore_errors=True)
            else:
                print("Cache command didn't return anything")

        run_cmd("sync", subfolder_path)
        run_cmd("plan", subfolder_path)
        library_path = run_cmd("library", subfolder_path)
        folder_count = len(os.listdir(library_path.strip()))

        if folder_count == 0:
            print(f"No folders found in library for {subfolder}")
            return 1

        test_folder = os.path.join(PARENT_FOLDER, subfolder, "tests")
        if os.path.exists(test_folder):
            run_r_test(library_path, test_folder)
            
            

    return 0

if __name__ == "__main__":
    exit(run_examples())