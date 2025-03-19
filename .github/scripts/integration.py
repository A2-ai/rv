import json
import os
import subprocess
import shutil

PARENT_FOLDER = "example_projects"

def run_cmd(cmd, path, json = False):
    additional_args = ["--json"] if json else []
    print(f"=== Running rv {cmd} ===")
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


def run_examples():
    items = os.listdir(PARENT_FOLDER)
    for subfolder in items:
        # This one needs lots of system deps, skipping in CI
        if subfolder == "big":
            continue
        subfolder_path = os.path.join(PARENT_FOLDER, subfolder, "rproject.toml")
        print(f"Processing example: {subfolder_path}")

        # The git packages depend on each other but we don't want
        if "git" in subfolder_path:
            cache_data = json.loads(run_cmd("cache", subfolder_path, ["--json"]))["git"]
            for obj in cache_data:
                shutil.rmtree(obj["source_path"], ignore_errors=True)
                shutil.rmtree(obj["binary_path"], ignore_errors=True)

        run_cmd("sync", subfolder_path)
        run_cmd("plan", subfolder_path)
        library_path = run_cmd("library", subfolder_path)
        folder_count = len(os.listdir(library_path.strip()))

        if folder_count == 0:
            print(f"No folders found in library for {subfolder}")
            return 1

    return 0

if __name__ == "__main__":
    exit(run_examples())