#!/usr/bin/env bash
# Verifie qu'une build de sshpass-gui demarre reellement: la fenetre s'ouvre, les
# polices systeme se chargent, la boucle de rendu tourne. Un binaire qui
# compile mais panique au lancement passerait sinon inapercu.
#
# Usage: smoke-test.sh <chemin/vers/executable>
set -euo pipefail

binary=${1:?usage: smoke-test.sh <executable>}
seconds=${SMOKE_SECONDS:-15}

# Configuration jetable: le test ne doit dependre d'aucun profil existant, ni
# laisser de trace derriere lui.
workdir=$(mktemp -d)
export SSHPASS_GUI_CONFIG="$workdir/config.toml"
printf 'version = 1\n' > "$SSHPASS_GUI_CONFIG"

pid=""
cleanup() {
    # Le groupe entier, pas seulement `xvfb-run`: l'application est son
    # petit-fils. Ne tuer que le pere la laisserait vivante, et comme elle
    # herite de la sortie standard, un appel redirige vers un tube n'en
    # verrait jamais la fin.
    if [ -n "$pid" ]; then
        kill -- -"$pid" 2>/dev/null || true
    fi
    rm -rf "$workdir"
}
trap cleanup EXIT

echo "Demarrage de $binary pendant $seconds s sur un serveur X virtuel..."
# `set -m` place chaque tache de fond dans son propre groupe de processus, dont
# l'identifiant est celui de la tache: c'est ce qui rend possible le
# `kill -- -$pid` ci-dessus.
set -m
xvfb-run -a --server-args="-screen 0 1280x800x24" "$binary" &
pid=$!
set +m

sleep "$seconds"

if kill -0 "$pid" 2>/dev/null; then
    echo "L'application tourne toujours apres $seconds s: demarrage correct."
    exit 0
fi

wait "$pid" 2>/dev/null && status=0 || status=$?
echo "::error::L'application s'est arretee pendant le test de demarrage (code $status)."
exit 1
