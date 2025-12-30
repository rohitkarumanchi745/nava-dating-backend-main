"""
Lightweight Postgres helper using psycopg2 (sync).
"""
import os
from typing import Any, Iterable, Optional

try:
    import psycopg2
    from psycopg2.pool import SimpleConnectionPool
    from psycopg2.extras import Json, RealDictCursor, NamedTupleCursor
except ImportError:  # pragma: no cover
    psycopg2 = None
    SimpleConnectionPool = None
    Json = None

DATABASE_URL = os.getenv("DATABASE_URL")
pg_pool: Optional[SimpleConnectionPool] = None

if psycopg2 and DATABASE_URL:
    try:
        pg_pool = SimpleConnectionPool(1, 5, dsn=DATABASE_URL)
    except Exception:
        pg_pool = None


def pg_execute(
    query: str,
    params: Optional[Iterable[Any]] = None,
    *,
    fetchone: bool = False,
    fetchall: bool = False,
    cursor_factory=None,
    commit: bool = True,
):
    if not pg_pool:
        raise RuntimeError("Postgres pool not initialized")
    conn = pg_pool.getconn()
    try:
        with conn.cursor(cursor_factory=cursor_factory) as cur:
            cur.execute(query, params)
            if fetchone:
                result = cur.fetchone()
                if commit:
                    conn.commit()
                return result
            if fetchall:
                result = cur.fetchall()
                if commit:
                    conn.commit()
                return result
            if commit:
                conn.commit()
            return None
    finally:
        pg_pool.putconn(conn)

# Convenience helpers for callers that want named/dict rows without importing psycopg2 extras
def pg_fetchone_dict(query: str, params: Optional[Iterable[Any]] = None):
    return pg_execute(query, params, fetchone=True, cursor_factory=RealDictCursor)


def pg_fetchall_dict(query: str, params: Optional[Iterable[Any]] = None):
    return pg_execute(query, params, fetchall=True, cursor_factory=RealDictCursor)


def pg_fetchone_namedtuple(query: str, params: Optional[Iterable[Any]] = None):
    return pg_execute(query, params, fetchone=True, cursor_factory=NamedTupleCursor)


def pg_fetchall_namedtuple(query: str, params: Optional[Iterable[Any]] = None):
    return pg_execute(query, params, fetchall=True, cursor_factory=NamedTupleCursor)
