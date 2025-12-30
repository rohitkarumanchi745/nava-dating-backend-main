#!/usr/bin/env python3
"""
Export a Postgres table to JSON for downstream Toon conversion.

Usage:
  python scripts/export_postgres_to_json.py <table_name> [output.json]

Environment:
  DATABASE_URL (e.g., postgresql://user:pass@host:5432/dbname)
"""

import json
import os
import sys
import psycopg2


def main():
    if len(sys.argv) < 2:
        print("Usage: python scripts/export_postgres_to_json.py <table_name> [output.json]")
        sys.exit(1)

    table = sys.argv[1]
    output = sys.argv[2] if len(sys.argv) > 2 else f"{table}.json"
    db_url = os.getenv("DATABASE_URL")
    if not db_url:
        print("DATABASE_URL is not set")
        sys.exit(1)

    conn = psycopg2.connect(db_url)
    cur = conn.cursor()
    cur.execute(f"SELECT * FROM {table}")
    rows = cur.fetchall()
    colnames = [desc[0] for desc in cur.description]

    data = [dict(zip(colnames, row)) for row in rows]

    with open(output, "w", encoding="utf-8") as f:
        json.dump(data, f, default=str, indent=2)

    cur.close()
    conn.close()

    print(f"Exported {len(data)} rows from {table} to {output}")


if __name__ == "__main__":
    main()
