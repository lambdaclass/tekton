import subprocess, json, base64, os

base_ref = "__BASE_REF__"
os.chdir("/home/agent/repo")
subprocess.run(["git", "add", "-A"], check=True)
if subprocess.run(["git", "diff", "--cached", "--quiet"]).returncode != 0:
    subprocess.run(["git", "commit", "-m", "temp"], check=True)
result = subprocess.run(
    ["git", "diff", "--name-status", base_ref + "..HEAD"],
    capture_output=True, text=True, check=True)
changes = {"additions": [], "deletions": []}
for line in result.stdout.strip().split("\n"):
    if not line: continue
    parts = line.split("\t")
    status = parts[0][0]
    if status == "D":
        changes["deletions"].append({"path": parts[1]})
    elif status == "R":
        changes["deletions"].append({"path": parts[1]})
        with open(parts[2], "rb") as f:
            changes["additions"].append({"path": parts[2], "contents": base64.b64encode(f.read()).decode()})
    else:
        with open(parts[-1], "rb") as f:
            changes["additions"].append({"path": parts[-1], "contents": base64.b64encode(f.read()).decode()})
print(json.dumps(changes))
