Install:

    sudo cp thehideout-backup.service thehideout-backup.timer /etc/systemd/system/
    sudo systemctl daemon-reload
    sudo systemctl enable --now thehideout-backup.timer
    systemctl list-timers thehideout-backup.timer
    journalctl -u thehideout-backup.service
