import os
import subprocess

PARENT_FOLDER = "example_projects"

def run_cmd(cmd, path):
    print(f"Running {cmd}")
    command = ["./target/release/rv", cmd, "--config-file", path, "-vvv"]
    result = subprocess.run(command, capture_output=True, text=True)
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