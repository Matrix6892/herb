#!/bin/sh
# Daily copy of the database and data/ to object storage, kept 90 days (spec §9).
# Needs sqlite3 and rclone; VITRINA_BACKUP_REMOTE is an rclone remote:path.
set -eu
: "${VITRINA_BACKUP_REMOTE:?set VITRINA_BACKUP_REMOTE}"
days="${VITRINA_BACKUP_DAYS:-90}"
db="${VITRINA_DB:-var/vitrina.db}"
stamp=$(date -u +%Y-%m-%dT%H%MZ)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# .backup takes a consistent copy while edge-api keeps writing.
sqlite3 "$db" ".backup '$work/vitrina-$stamp.db'"
gzip -9 "$work/vitrina-$stamp.db"
tar -czf "$work/data-$stamp.tar.gz" data
rclone copy "$work" "$VITRINA_BACKUP_REMOTE/$stamp/"
rclone delete --min-age "${days}d" "$VITRINA_BACKUP_REMOTE/"
rclone rmdirs --leave-root "$VITRINA_BACKUP_REMOTE/" || true
