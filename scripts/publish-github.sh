#!/usr/bin/env bash
# publish-github.sh — unggah discovery_X ke GitHub DENGAN AMAN.
#
# Mencegah kebocoran rahasia (API key, hash password admin, database temuan,
# scope target) dengan memeriksa staging area SEBELUM commit/push:
#   1. memastikan .gitignore memuat entri rahasia penting;
#   2. menolak bila ada file rahasia yang ter-stage;
#   3. memindai isi yang di-stage untuk pola rahasia (sk-…, $argon2id$…).
#
# Pemakaian:
#   scripts/publish-github.sh -m "pesan commit" [--remote git@github.com:user/repo.git]
#   scripts/publish-github.sh --check        # hanya audit, tanpa commit
set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || dirname "$(dirname "$(readlink -f "$0")")")"

MSG="initial commit: discovery_X"
REMOTE=""
CHECK_ONLY=0
BRANCH="main"

while [[ $# -gt 0 ]]; do
  case "$1" in
    -m|--message) MSG="$2"; shift 2 ;;
    --remote)     REMOTE="$2"; shift 2 ;;
    --branch)     BRANCH="$2"; shift 2 ;;
    --check)      CHECK_ONLY=1; shift ;;
    *) echo "argumen tak dikenal: $1" >&2; exit 2 ;;
  esac
done

red()  { printf '\033[31m%s\033[0m\n' "$*"; }
grn()  { printf '\033[32m%s\033[0m\n' "$*"; }
ylw()  { printf '\033[33m%s\033[0m\n' "$*"; }

# File/pola yang TIDAK BOLEH pernah masuk ke repo.
FORBIDDEN_FILES=(
  config.toml config.local.toml scope.txt
  discovery.db discovery.db-wal discovery.db-shm
  .env attack-graph.dot
)
FORBIDDEN_GLOBS='\.(db|db-wal|db-shm|pem|key)$|(^|/)\.env(\.|$)|(^|/)secrets'
# Pola isi rahasia (heuristik konservatif). Sengaja ketat agar placeholder
# dokumentasi (mis. "$argon2id$..." atau "sk-...") TIDAK ikut tertangkap —
# hanya nilai sungguhan: hash Argon2 lengkap (ada salt+hash base64) & API key nyata.
SECRET_CONTENT='\$argon2(id|i|d)\$v=[0-9]+\$m=[0-9]+,t=[0-9]+,p=[0-9]+\$[A-Za-z0-9+/]{16,}|sk-[A-Za-z0-9]{20,}'

# ── 0. pastikan repo git ──────────────────────────────────────────────
if [[ ! -d .git ]]; then
  if [[ $CHECK_ONLY -eq 1 ]]; then
    ylw "Belum ada repo git (mode --check: tidak menginisialisasi)."
  else
    ylw "Tidak ada repo git — inisialisasi (branch $BRANCH)…"
    git init -b "$BRANCH" >/dev/null
  fi
fi

# ── 1. pastikan .gitignore melindungi rahasia ─────────────────────────
miss=()
for e in config.toml scope.txt discovery.db .env; do
  grep -qxF "$e" .gitignore 2>/dev/null || grep -qE "(^|/)$(printf '%s' "$e" | sed 's/\./\\./g')(\$|\b)" .gitignore 2>/dev/null || miss+=("$e")
done
if [[ ${#miss[@]} -gt 0 ]]; then
  red "✗ .gitignore tidak melindungi: ${miss[*]}"
  red "  Tambahkan dulu sebelum publish."
  exit 1
fi
grn "✓ .gitignore memuat entri rahasia penting"

# ── 2. stage & audit ──────────────────────────────────────────────────
if [[ $CHECK_ONLY -eq 0 ]]; then
  git add -A
fi
# Daftar file yang akan masuk repo (staged) atau sudah terlacak.
mapfile -t TRACKED < <(git diff --cached --name-only 2>/dev/null; git ls-files 2>/dev/null)
TRACKED=($(printf '%s\n' "${TRACKED[@]}" | sort -u))

leak=0
for f in "${TRACKED[@]}"; do
  [[ -z "$f" ]] && continue
  for bad in "${FORBIDDEN_FILES[@]}"; do
    [[ "$f" == "$bad" ]] && { red "✗ file rahasia ter-stage: $f"; leak=1; }
  done
  if [[ "$f" =~ $FORBIDDEN_GLOBS ]]; then
    red "✗ file cocok pola rahasia: $f"; leak=1
  fi
done

# Pindai ISI yang akan di-commit untuk pola rahasia.
if git rev-parse --verify -q HEAD >/dev/null 2>&1 || git diff --cached --quiet 2>/dev/null; then :; fi
if content_hits=$(git diff --cached -U0 2>/dev/null | grep -nE "$SECRET_CONTENT" || true); [[ -n "${content_hits:-}" ]]; then
  red "✗ ditemukan pola rahasia di isi yang akan di-commit:"
  printf '%s\n' "$content_hits" | sed 's/^/    /'
  leak=1
fi

if [[ $leak -ne 0 ]]; then
  red ""
  red "DIBATALKAN demi keamanan. Hapus file/rahasia di atas dari staging:"
  red "  git rm --cached <file>   # lalu pastikan ada di .gitignore"
  [[ $CHECK_ONLY -eq 0 ]] && git reset -q
  exit 1
fi
grn "✓ tidak ada file/isi rahasia yang akan diunggah"

if [[ $CHECK_ONLY -eq 1 ]]; then
  grn "Audit selesai — aman untuk publish."
  exit 0
fi

# ── 3. commit ─────────────────────────────────────────────────────────
if git diff --cached --quiet; then
  ylw "Tidak ada perubahan untuk di-commit."
else
  git commit -q -m "$MSG"
  grn "✓ commit dibuat: $MSG"
fi

# ── 4. remote & push ──────────────────────────────────────────────────
if [[ -n "$REMOTE" ]]; then
  if git remote get-url origin >/dev/null 2>&1; then
    git remote set-url origin "$REMOTE"
  else
    git remote add origin "$REMOTE"
  fi
fi

if git remote get-url origin >/dev/null 2>&1; then
  git branch -M "$BRANCH"
  ylw "Push ke origin/$BRANCH…"
  git push -u origin "$BRANCH"
  grn "✓ terkirim ke GitHub."
else
  ylw "Belum ada remote 'origin'. Buat repo KOSONG di GitHub lalu:"
  echo "    git remote add origin git@github.com:USER/REPO.git"
  echo "    git branch -M $BRANCH && git push -u origin $BRANCH"
  echo "  atau ulangi: scripts/publish-github.sh -m \"$MSG\" --remote git@github.com:USER/REPO.git"
fi
