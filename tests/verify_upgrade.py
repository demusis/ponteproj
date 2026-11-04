"""Read-only verification: existing encrypted records survived migration."""
from contextlib import closing
from pathlib import Path
import sqlite3
import sys

with closing(sqlite3.connect(Path(sys.argv[1]).resolve().as_uri() + '?mode=ro', uri=True)) as old:
    with closing(sqlite3.connect(Path(sys.argv[2]).resolve().as_uri() + '?mode=ro', uri=True)) as current:
        assert current.execute('PRAGMA user_version').fetchone()[0] == 9
        assert current.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
        assert current.execute('PRAGMA foreign_key_check').fetchall() == []
        for table in ('captures', 'notes', 'drafts', 'analyses', 'tasks'):
            for row_id, payload in old.execute(f'SELECT id,payload FROM {table}'):
                assert current.execute(f'SELECT payload FROM {table} WHERE id=?', (row_id,)).fetchone() == (payload,)
        for doc_id, payload in old.execute('SELECT document_id,payload FROM work_context'):
            assert current.execute('SELECT payload FROM work_context WHERE document_id=?', (doc_id,)).fetchone() == (payload,)
        missing = current.execute('SELECT count(*) FROM documents d WHERE NOT EXISTS (SELECT 1 FROM tasks t WHERE t.document_id=d.id)').fetchone()[0]
        assert missing == 0
        print('Upgrade OK: original encrypted records preserved; integrity and links valid.')
        print('Documents:', current.execute('SELECT count(*) FROM documents').fetchone()[0])
        print('Captures:', current.execute('SELECT count(*) FROM captures').fetchone()[0])
        print('Tasks:', current.execute('SELECT count(*) FROM tasks').fetchone()[0])
