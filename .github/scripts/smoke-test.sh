#!/usr/bin/env bash
# Verifie qu'une build de sshpass demarre reellement: la fenetre s'ouvre, les
# polices systeme se chargent, la boucle de rendu tourne. Un binaire qui
# compile mais panique au lancement passerait sinon inapercu.
#
# Usage: smoke-test.sh <chemin/vers/executable>
set -euo pipefail

binary=${1:?usage: smoke-test.sh <executable>}
seconds=${SMOKE_SECONDS:-15}

# Configuration jetable: le test ne doit pas dependre du profil de la machine,
# ni laisser de trace derriere lui.
workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT
printf 'version = 1\n' > "$workdir/config.toml"
export SSHPASS_CONFIG="$workdir/config.toml"

echo "Demarrage de $binary pendant $seconds s sur un serveur X virtuel..."
xvfb-run -a --server-args="-screen 0 1280x800x24" "$binary" &
pid=$!

sleep "$seconds"

if kill -0 "$pid" 2>/dev/null; then
    echo "L'application tourne toujours apres $seconds s: demarrage correct."
    kill "$pid"
    wait "$pid" 2>/dev/null || true
    exit 0
fi

wait "$pid" 2>/dev/null && status=0 || status=$?
echo "::error::L'application s'est arretee pendant le test de demarrage (code $status)."
exit 1
