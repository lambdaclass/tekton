import os
from flask import Flask

app = Flask(__name__)


@app.route("/")
def hello():
    branch = os.environ.get("PREVIEW_BRANCH", "unknown")
    host = os.environ.get("PREVIEW_HOST", "localhost")
    return (
        "<html><body>"
        "<h1>Hello, World!</h1>"
        f"<p>Branch: <code>{branch}</code></p>"
        f"<p>Host: <code>{host}</code></p>"
        "</body></html>"
    )


if __name__ == "__main__":
    port = int(os.environ.get("PORT", 8000))
    app.run(host="0.0.0.0", port=port)
