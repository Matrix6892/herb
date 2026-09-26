#!/bin/sh
# Health checks (spec §9): the newest snapshot is under 36 hours old, the
# category page answers, and edge-api is up. Mails the owner on failure.
set -u
problems=""
newest=$(ls -1t var/snapshots/vitrina-*.db 2>/dev/null | head -n 1)
if [ -z "$newest" ]; then
    problems="$problems\n- no snapshot in var/snapshots"
else
    age=$(( $(date +%s) - $(stat -c %Y "$newest") ))
    if [ "$age" -gt $((36 * 3600)) ]; then
        problems="$problems\n- newest snapshot $newest is $((age / 3600)) hours old"
    fi
fi
site="${VITRINA_SITE_URL:?}/c/${VITRINA_CATEGORY_SLUG:-magnesium}"
code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 "$site")
[ "$code" = "200" ] || problems="$problems\n- $site answered $code"
# A GET is refused with 405 by a live edge-api and writes nothing.
api="${VITRINA_SITE_URL}/api/click"
code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 "$api")
[ "$code" = "405" ] || problems="$problems\n- $api answered $code (expected 405)"
if [ -n "$problems" ]; then
    msg=$(printf "Vitrina health check failed:%b\n" "$problems")
    echo "$msg" >&2
    if [ -n "${VITRINA_NOTIFY_EMAIL:-}" ]; then
        printf 'Subject: vitrina: health check failed\nTo: %s\n\n%s\n' "$VITRINA_NOTIFY_EMAIL" "$msg" | /usr/sbin/sendmail -t
    fi
    exit 1
fi
