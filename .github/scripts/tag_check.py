import re
import sys
import subprocess



def verify_tag(git_tag):
    result = subprocess.run(
        ["cargo", "run", "--features=cli", "--release", "--", "--version"],
        capture_output=True,
        text=True,
        check=True,
    )

    version = f"v{result.stdout.replace('rv', '').strip()}"
    if git_tag != version:
        print(f"Different version compared to tag: tag={git_tag} cli={version}")
        return 1

    return 0


if __name__ == "__main__":
    exit(verify_tag(sys.argv[1]))