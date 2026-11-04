"""Consistent local SQLite backup before upgrading this user's encrypted store."""
import datetime
import os
from pathlib import Path
import sqlite3
import sys
import re

version = sys.argv[1] if len(sys.argv) > 1 else '0.3.0'
assert re.fullmatch(r'\d+\.\d+\.\d+', version)

directory = Path(os.environ["LOCALAPPDATA"]) / "PonteProj"
source = directory / "acervo.sqlite3"
if source.exists():
    backups = directory / "backups"
    backups.mkdir(exist_ok=True)
    target = backups / ("pre-v" + version.replace('.', '') + '-' + datetime.datetime.now().strftime("%Y%m%d-%H%M%S-%f") + ".sqlite3")
    with sqlite3.connect(source.as_uri() + "?mode=ro", uri=True) as original:
        with sqlite3.connect(target) as backup:
            original.backup(backup)
            assert backup.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
    print("Backup consistente criado:", target)
else:
    print("Sem acervo anterior para copiar.")
