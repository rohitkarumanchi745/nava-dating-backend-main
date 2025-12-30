.PHONY: install run fmt lint test

install:
\tpython3 -m venv .venv || true
\t. .venv/bin/activate && pip install -r requirements.txt

run:
\t. .venv/bin/activate && uvicorn main:app --reload --host 0.0.0.0 --port 8000

fmt:
\t. .venv/bin/activate && black . && isort .

lint:
\t. .venv/bin/activate && black --check . && isort --check-only .

test:
\tpytest -q
