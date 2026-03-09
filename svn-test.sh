#!/bin/bash

# Configuration
REPO_DIR="$HOME/.tmp_svn_repo"
WC_DIR=$(pwd)/test_wc

echo "--- SVN Test Environment Manager ---"

# 1. Create the Repo if it doesn't exist
if [ ! -d "$REPO_DIR" ]; then
    echo "[+] Creating SVN Repository at $REPO_DIR"
    mkdir -p "$REPO_DIR"
    svnadmin create "$REPO_DIR"
fi

# 2. Create the Working Copy if it doesn't exist
if [ ! -d "$WC_DIR" ]; then
    echo "[+] Checking out Working Copy to $WC_DIR"
    svn checkout "file://$REPO_DIR" "$WC_DIR"
fi

cd "$WC_DIR" || exit

# 3. Generate random changes to test your Rust TUI
echo "[+] Generating test changes..."

# Case A: Modified File
if [ ! -f "stable_file.txt" ]; then
    echo "Initial content" > stable_file.txt
    svn add stable_file.txt
    svn commit -m "Add stable file"
fi
echo "New change at $(date +%H:%M:%S)" >> stable_file.txt

# Case B: Newly Added File (Status 'A')
NEW_FILE="feature_$(date +%s).rs"
echo "// Experimental Rust code" > "$NEW_FILE"
svn add "$NEW_FILE"

# Case C: Deleted File (Status 'D')
if [ ! -f "delete_me.txt" ]; then
    echo "temp" > delete_me.txt
    svn add delete_me.txt
    svn commit -m "Preparing a file for deletion"
fi
svn rm delete_me.txt --force

# Case D: Untracked File (Status '?')
echo "log data" > "untracked_$(date +%s).log"

echo "--- Done! ---"
echo "Run your Rust TUI inside: $WC_DIR"
svn status
