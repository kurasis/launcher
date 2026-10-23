#!/usr/bin/env bash
# What the lapi server is short of when Cloudflare cannot reach it (525, 52x,
# slow files): load, memory, disk, nginx's limits, TCP queues and TLS from the
# server itself. Run over SSH by "Update service" (task "server"):
#   ssh "$TARGET" bash -s < scripts/server-check.sh
# The repository and its logs are public: this prints fixed words and numbers
# only, never an address, a name of a player or a line of a log.
set -u

row() { printf '%-34s %s\n' "$1" "$2"; }
num() { awk -v k="$1" '$1 == k { gsub(";", "", $2); print $2; exit }' "$2" 2>/dev/null; }

echo "== machine"
row cpus "$(nproc)"
row load_1_5_15 "$(cut -d' ' -f1-3 /proc/loadavg)"
row mem_total_mb "$(awk '/^MemTotal:/ { print int($2 / 1024) }' /proc/meminfo)"
row mem_available_mb "$(awk '/^MemAvailable:/ { print int($2 / 1024) }' /proc/meminfo)"
row swap_used_mb "$(awk '/^SwapTotal:/ { t = $2 } /^SwapFree:/ { f = $2 } END { print int((t - f) / 1024) }' /proc/meminfo)"
row disk_root_used_pct "$(df --output=pcent / | tail -1 | tr -dc 0-9)"
row disk_lapi_used_pct "$(df --output=pcent /home/www/lapi 2>/dev/null | tail -1 | tr -dc 0-9)"
row uptime_hours "$(awk '{ print int($1 / 3600) }' /proc/uptime)"

echo "== nginx"
conf=/etc/nginx/nginx.conf
row worker_processes "$(num worker_processes "$conf")"
row worker_connections "$(num worker_connections "$conf")"
row worker_rlimit_nofile "$(num worker_rlimit_nofile "$conf" | grep . || echo unset)"
workers=$(pgrep -f 'nginx: worker process' | wc -l)
row workers_running "$workers"
w=$(pgrep -f 'nginx: worker process' | head -1)
if [ -n "$w" ]; then
    row worker_open_files_limit "$(awk '/^Max open files/ { print $4 }' "/proc/$w/limits")"
fi
row nginx_active "$(systemctl is-active nginx 2>/dev/null)"
since=$(systemctl show nginx -p ActiveEnterTimestamp --value 2>/dev/null)
[ -n "$since" ] && row nginx_running_hours "$(( ($(date +%s) - $(date -d "$since" +%s)) / 3600 ))"

echo "== tcp"
count() { ss -Htn state "$1" "( sport = :443 )" 2>/dev/null | wc -l; }
row https_established "$(count established)"
row https_syn_recv "$(count syn-recv)"
row https_time_wait "$(count time-wait)"
row https_close_wait "$(count close-wait)"
row all_established "$(ss -Htn state established 2>/dev/null | wc -l)"
# Recv-Q of a listening socket is its accept queue now, Send-Q its size.
ss -Hltn '( sport = :443 )' 2>/dev/null | awk '{ printf "%-34s %s of %s\n", "https_accept_queue", $2, $3 }'
tcpext() { awk -v k="$1" '$1 == "TcpExt:" && !h { for (i = 2; i <= NF; i++) n[i] = $i; h = 1; next }
    $1 == "TcpExt:" { for (i = 2; i <= NF; i++) if (n[i] == k) print $i }' /proc/net/netstat; }
for k in ListenOverflows ListenDrops TCPAbortOnMemory TCPAbortOnTimeout TCPReqQFullDrop TCPBacklogDrop; do
    row "$k (since boot)" "$(tcpext "$k")"
done
awk '$1 == "Tcp:" && !h { for (i = 2; i <= NF; i++) n[i] = $i; h = 1; next }
    $1 == "Tcp:" { for (i = 2; i <= NF; i++) if (n[i] ~ /^(AttemptFails|EstabResets|RetransSegs|OutSegs)$/) printf "%-34s %s\n", n[i] " (since boot)", $i }' /proc/net/snmp
if [ -r /proc/sys/net/netfilter/nf_conntrack_count ]; then
    row conntrack "$(cat /proc/sys/net/netfilter/nf_conntrack_count) of $(cat /proc/sys/net/netfilter/nf_conntrack_max)"
else
    row conntrack "not loaded"
fi
row somaxconn "$(cat /proc/sys/net/core/somaxconn)"
row tcp_max_syn_backlog "$(cat /proc/sys/net/ipv4/tcp_max_syn_backlog)"
row global_ipv6_addresses "$(ip -6 addr show scope global 2>/dev/null | grep -c inet6)"
for unit in fail2ban ufw nftables crowdsec; do
    row "$unit" "$(systemctl is-active "$unit" 2>/dev/null)"
done

echo "== network, 10 seconds"
iface=$(ip route show default 2>/dev/null | awk '{ print $5; exit }')
if [ -n "$iface" ]; then
    s=/sys/class/net/$iface/statistics
    rx=$(cat "$s/rx_bytes") tx=$(cat "$s/tx_bytes") rd=$(cat "$s/rx_dropped") re=$(cat "$s/rx_errors")
    sleep 10
    row in_mbit_s "$(( ($(cat "$s/rx_bytes") - rx) * 8 / 10000000 ))"
    row out_mbit_s "$(( ($(cat "$s/tx_bytes") - tx) * 8 / 10000000 ))"
    row rx_dropped_10s "$(( $(cat "$s/rx_dropped") - rd ))"
    row rx_errors_10s "$(( $(cat "$s/rx_errors") - re ))"
    row link_mbit "$(cat "/sys/class/net/$iface/speed" 2>/dev/null || echo unknown)"
fi

echo "== busiest processes (name, cpu %, memory %)"
ps -eo comm=,pcpu=,pmem= --sort=-pcpu | head -8 | awk '{ printf "%-34s %s %s\n", $1, $2, $3 }'
echo "nginx_processes $(pgrep -c nginx)"

echo "== nginx error logs: lines per kind (whole file)"
kinds=(
    'worker_connections are not enough'
    'Too many open files'
    'SSL_do_handshake() failed'
    'upstream timed out'
    'no live upstreams'
    'Cannot allocate memory'
)
for f in /var/log/nginx/error.log /var/log/nginx/*.error.log; do
    [ -e "$f" ] || continue
    if [ ! -r "$f" ]; then row "$(basename "$f")" "not readable"; continue; fi
    line="lines=$(wc -l < "$f")"
    for k in "${kinds[@]}"; do
        c=$(grep -cF -- "$k" "$f")
        [ "$c" -gt 0 ] && line="$line; $k=$c"
    done
    row "$(basename "$f")" "$line"
done

echo "== upd access log: answers per status, last 20000 requests"
log=/var/log/nginx/upd.lapi.sh.access.log
if [ -r "$log" ]; then
    tail -n 20000 "$log" | awk '{ for (i = 1; i < NF; i++) if ($i ~ /"$/ && $(i + 1) ~ /^[0-9][0-9][0-9]$/) { print $(i + 1); break } }' |
        sort | uniq -c | sort -rn | awk '{ printf "%-34s %s\n", "status " $2, $1 }'
else
    row upd_access_log "not readable"
fi

echo "== kernel, last 12 hours"
if journalctl -k -q --since -12h -n 1 >/dev/null 2>&1; then
    k=$(journalctl -k -q --since -12h 2>/dev/null)
    for w in 'SYN flooding' 'nf_conntrack: table full' 'Out of memory' 'TCP: out of memory' 'link is not ready' 'NETDEV WATCHDOG'; do
        row "$w" "$(grep -cF -- "$w" <<< "$k")"
    done
else
    row kernel_log "not readable"
fi

echo "== TLS from the server itself (upd.lapi.sh on 127.0.0.1)"
pem=$(echo | timeout 10 openssl s_client -connect 127.0.0.1:443 -servername upd.lapi.sh 2>/dev/null | openssl x509 2>/dev/null)
if [ -n "$pem" ]; then
    end=$(openssl x509 -noout -enddate <<< "$pem" | cut -d= -f2)
    row certificate_days_left "$(( ($(date -d "$end" +%s) - $(date +%s)) / 86400 ))"
    if openssl x509 -noout -issuer <<< "$pem" | grep -q "Let's Encrypt"; then
        row certificate "lets-encrypt"
    elif [ "$(openssl x509 -noout -issuer <<< "$pem" | cut -d= -f2-)" = "$(openssl x509 -noout -subject <<< "$pem" | cut -d= -f2-)" ]; then
        row certificate "self-signed"
    else
        row certificate "other"
    fi
else
    row certificate "no TLS answer"
fi
ok=0 bad=0 total_ms=0 slow=0
for _ in $(seq 1 40); do
    t=$(curl --noproxy '*' -so /dev/null -w '%{http_code} %{time_appconnect}' --max-time 10 \
        --resolve upd.lapi.sh:443:127.0.0.1 https://upd.lapi.sh/v1/stable/windows-x86_64-nsis.json 2>/dev/null)
    case "$t" in
        200\ *|204\ *)
            ok=$((ok + 1))
            ms=$(awk '{ print int($2 * 1000) }' <<< "$t")
            total_ms=$((total_ms + ms))
            [ "$ms" -gt 500 ] && slow=$((slow + 1)) ;;
        *) bad=$((bad + 1)) ;;
    esac
done
row local_https_ok "$ok of 40"
row local_https_failed "$bad"
[ "$ok" -gt 0 ] && row local_tls_handshake_avg_ms "$((total_ms / ok))"
row local_tls_handshake_over_500ms "$slow"
