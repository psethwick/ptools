#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "prompt_toolkit>=3.0",
# ]
# ///

from prompt_toolkit import prompt
import sqlite3
from pprint import pp

working_on = """select
  w.project,
  p."name",
  w.title, column
from
  work w
  join person p on p.source_id = w.source_id
  and w.assigned_to_id = p.id
  where state = 'Active' """


def run_sql(query: str):
    conn = sqlite3.connect("/home/chicken/.local/share/psync/psync.db")
    cursor = conn.cursor()
    cursor.execute(query)
    results = cursor.fetchall()
    conn.close()
    return results


commands = ["user", "exit", "project"]

if __name__ == "__main__":
    while True:
        answer = prompt(": ")
        if answer.startswith("user"):
            results = run_sql(
                working_on + f"and p.name like '%{answer.replace('user', '').strip()}%'"
            )
            pp(results)
        elif answer.startswith("project"):
            results = run_sql(
                working_on
                + f"and w.project like '%{answer.replace('project', '').strip()}%'"
            )
        elif answer.startswith("exit"):
            exit(0)
