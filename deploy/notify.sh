#!/bin/sh
# Mails the owner the tail of a failed unit's log. Usage: notify.sh <unit|message>
set -u
subject="vitrina: $1 failed on $(hostname)"
body=$(journalctl -u "$1" -n 60 --no-pager 2>/dev/null || echo "$1")
if [ -z "${VITRINA_NOTIFY_EMAIL:-}" ]; then
    echo "$subject" >&2
    echo "$body" >&2
    exit 0
fi
printf 'Subject: %s\nTo: %s\n\n%s\n' "$subject" "$VITRINA_NOTIFY_EMAIL" "$body" | /usr/sbin/sendmail -t
