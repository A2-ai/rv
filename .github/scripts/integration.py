import os
import subprocess

PARENT_FOLDER = "example_projects"

def run_examples():
    items = os.listdir(PARENT_FOLDER)
    for subfolder in items:
        # This one needs lots of system deps, skipping in CI
        if subfolder == "big":
            continue
        subfolder_path = os.path.join(PARENT_FOLDER, subfolder, "rproject.toml")
        print(f"Processing example: {subfolder_path}")

        command = ["./target/release/rv", "sync", "--config-file", subfolder_path]
        result = subprocess.run(command, capture_output=True, text=True)
        print(result.stdout)

        # Check for errors
        if result.returncode != 0:
            print(f"Command failed with error: {result.stderr}")
            return 1

    return 0

if __name__ == "__main__":
    exit(run_examples())