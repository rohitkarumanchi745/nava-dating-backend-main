# Telugu Dating Backend

## Setup
```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
uvicorn main:app --reload
```

## Notes
- Do **not** commit virtualenvs, DB files, or uploaded media.
- Large models/datasets should live outside Git (e.g., S3). If you must version them, use Git LFS selectively.
