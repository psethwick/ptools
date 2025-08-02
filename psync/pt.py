#!/usr/bin/env -S uv run
"""
thoughts

the tiered model of cli I thought I wanted I... don't want

the reason is the filters I want are orthogonal

but maybe we could have commands and state?
so like a filter command:
filter assigned seth
filter project cloudcheep

they could .. compose?
you could empty the assigned filter:
filter assigned

I'd like a filter for 'only recently changed stuff'
not sure what that looks like

"""
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "prompt_toolkit>=3.0",
# ]
# ///

from prompt_toolkit import prompt
import sqlite3
from pprint import pp

work = """select
  w.project,
  p."name",
  w.title, column
from
  work w
  join person p on p.source_id = w.source_id
  and w.assigned_to_id = p.id
  where 1=1 """

# state = 'Active'


def run_sql(query: str):
    conn = sqlite3.connect("/home/chicken/.local/share/psync/psync.db")
    cursor = conn.cursor()
    cursor.execute(query)
    results = cursor.fetchall()
    conn.close()
    return results


commands = ["user", "exit", "project"]

state_filter = "and state = 'Active' "
user_filter = "and 1=1 "
project_filter = "and 1=1 "

if __name__ == "__main__":
    while True:
        answer = prompt(": ")
        if answer.startswith("filter user"):
            f = answer.replace("filter user", "")
            user_filter = f"and p.name like '%{f}%'" if f else "and 1=1 "
        elif answer.startswith("filter project"):
            f = answer.replace("filter project", "")
            project_filter = (
                f"and w.project like '%{answer.replace('filter project', '').strip()}%'"
                if f
                else "and 1=1 "
            )

        elif answer.startswith("exit"):
            exit(0)

        results = run_sql(work + state_filter + user_filter + project_filter)
        pp(results)
