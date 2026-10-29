#!/bin/sh
set -e

if [ "$1" = "configure" ]; then
    # Auto-register Native Messaging Host for web browsers
    if [ -x /usr/bin/oxide-cli ]; then
        /usr/bin/oxide-cli install-browsers --nmh-path /usr/bin/oxide-nmh || true
    fi
    # Update icon cache and desktop database if tools are available
    if which update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q || true
    fi
fi

exit 0
