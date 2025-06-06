import json
import os
import subprocess
import shutil

PARENT_FOLDER = "example_projects"
SKIP_PLAN_CHECK = ["no-lockfile", "url", "custom-lib-path"]

def run_cmd(cmd, path, json = False):
    print(f">> Running rv {cmd}")
    command = ["./target/debug/rv", "--config-file", path, "-vvv"] + (["--json"] if json else []) + [cmd]
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
        if subfolder != "windows-carData":
            continue
        subfolder_path = os.path.join(PARENT_FOLDER, subfolder, "rproject.toml")
        print(f"===== Processing example: {subfolder_path} =====")

        # The git packages depend on each other but we don't want rv to use the cache for them
        if "git" in subfolder_path:
            out = run_cmd("cache", subfolder_path, True)
            if out:
                cache_data = json.loads(out)
                for obj in cache_data.get("git", []):
                    print(f"Clearing cache: {obj}")
                    shutil.rmtree(obj["source_path"], ignore_errors=True)
            else:
                print("Cache command didn't return anything")

        run_cmd("sync", subfolder_path)
        plan_result = run_cmd("plan", subfolder_path)
        if "Nothing to do" not in plan_result and not any([True for s in SKIP_PLAN_CHECK if s in subfolder_path]):
            print(f"Plan after sync has changes planned for {subfolder}")
            return 1
        library_path = run_cmd("library", subfolder_path)
        folder_count = len(os.listdir(library_path.strip()))

        if folder_count == 0:
            print(f"No folders found in library for {subfolder}")
            return 1

    return 0

if __name__ == "__main__":
    exit(run_examples())