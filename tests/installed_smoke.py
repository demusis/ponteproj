"""Exercise the packaged executable with fictitious data and an isolated store."""
import json
import os
from pathlib import Path
import struct
import sqlite3
import subprocess
import sys
import tempfile
import base64
import uuid
from contextlib import closing


def frame(value):
    payload = json.dumps(value, ensure_ascii=False).encode("utf-8")
    return struct.pack("<I", len(payload)) + payload


def unpack(data):
    replies = []
    while data:
        assert len(data) >= 4, "truncated native response"
        size = struct.unpack("<I", data[:4])[0]
        assert 0 < size <= 1024 * 1024 and len(data) >= size + 4
        replies.append(json.loads(data[4:4 + size]))
        data = data[4 + size:]
    return replies


def main():
    executable = Path(sys.argv[1]).resolve(strict=True)
    capture = json.loads((Path(__file__).parent / "fixtures/capture.json").read_text(encoding="utf-8"))
    # The inspected SIGA page identifies a via in its URL and the base in H2.
    capture["source_url"] += "-A"
    with tempfile.TemporaryDirectory(prefix="ponte-smoke-") as directory:
        environment = os.environ.copy()
        environment["PONTEPROJ_DATA_DIR"] = directory
        environment["PONTEPROJ_NO_UI"] = "1"
        request = {"protocol_version": 1, "action": "import_capture", "capture": capture}
        process = subprocess.run([str(executable), "--native-host"], input=frame(request) * 2,
                                 capture_output=True, env=environment, timeout=30)
        assert process.returncode == 0, f"native host exit={process.returncode}"
        replies = unpack(process.stdout)
        assert len(replies) == 2 and all(reply.get("ok") for reply in replies)
        assert replies[0]["document_id"] == replies[1]["document_id"]
        assert replies[0]["capture_id"] == replies[1]["capture_id"]
        assert replies[1]["duplicate"] is True
        capture["request_id"] = str(uuid.uuid4())
        file_url = 'https://www.sigadoc.mt.gov.br/arquivos/teste.pdf'
        capture['references'].append({'label':'PDF fictício', 'url':file_url, 'kind':'attachment'})
        request = {'protocol_version':1,'action':'import_bundle','capture':capture,'files':[{'url':file_url,'data_base64':base64.b64encode((Path(__file__).parent/'fixtures/document.pdf').read_bytes()).decode()}]}
        process = subprocess.run([str(executable), '--native-host'], input=frame(request)*2, capture_output=True, env=environment, timeout=60)
        assert process.returncode == 0
        bundle_replies = unpack(process.stdout)
        assert len(bundle_replies) == 2 and all(reply.get('ok') for reply in bundle_replies)
        assert bundle_replies[1]['duplicate'] is True
        listing = subprocess.run([str(executable), "--list", "fictícios"], capture_output=True,
                                 env=environment, timeout=30)
        assert listing.returncode == 0
        documents = json.loads(listing.stdout)
        assert len(documents) == 1 and documents[0]["document_number"] == capture["document_number"]
        with closing(sqlite3.connect(Path(directory) / "acervo.sqlite3")) as database:
            assert database.execute("PRAGMA user_version").fetchone()[0] == 9
            assert database.execute("SELECT count(*) FROM tasks").fetchone()[0] == 1
            assert database.execute("SELECT count(*) FROM files").fetchone()[0] == 1
            assert database.execute("PRAGMA foreign_key_check").fetchall() == []
        for path in Path(directory).glob("acervo.sqlite3*"):
            contents = path.read_bytes()
            assert capture["title"].encode("utf-8") not in contents
            assert capture["text"].encode("utf-8") not in contents
    print("Installed smoke OK: framing, via identity, replay, PDF bundle, encrypted store, linked task and schema v9.")


if __name__ == "__main__":
    main()
