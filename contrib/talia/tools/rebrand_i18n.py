"""Replace the app name "Liana" with "Talia Desktop" in chosen UI strings.

Usage: python3 -I rebrand_i18n.py <liana-i18n/i18n dir>

Only the entries listed in IDS change, in every language file (the English source
copy inside each translation file changes the same way, so the catalogs stay in sync).
Kept on purpose: "Liana Connect" (Wizardsardine's service), "Liana descriptor"
(the file format), the "Liana daemon" (the engine, credited in About) and the
"Liana-<checksum>" wallet ids.
"""
import re
import sys
from pathlib import Path

IDS = {
    "bitcoind-upgrade-hint",
    "decrypt-other-paths-hint",
    "device-unrelated-wallet",
    "hw-display-address-unavailable",
    "installer-auto-install-node",
    "installer-backup-descriptor-help",
    "installer-bitcoind-already-installed",
    "installer-existing-node-description",
    "installer-local-wallet-description",
    "installer-managed-node-description",
    "installer-remote-bitcoin-node-warning",
    "launcher-add-existing-wallet",
    "launcher-create-new-wallet",
    "launcher-default-wallet-name",
    "launcher-delete-node-not-affected",
    "launcher-delete-node-not-affected-this-network",
    "loader-failed",
    "loader-failed-bitcoind",
    "settings-import-export-description",
}

ENTRY_START = re.compile(r"^([a-z0-9][a-z0-9-]*)(\s|$)")


def rebrand(line: str) -> str:
    line = line.replace("Liana-managed", "Talia Desktop managed")
    return re.sub(r"(?:(?<=\\n)|\b)Liana\b(?![- ]Connect)", "Talia Desktop", line)


# 21st Capital wording rule: client text says "signing device", never
# "hardware wallet". Liana's sentences already say "signing device" and add
# the other name as an aside, so the aside is dropped (every language).
WORDING = [
    (", commonly known as a hardware wallet,", ""),
    (", communément appelé hardware wallet,", ""),
    (", comunemente noto come hardware wallet,", ""),
    (" (também conhecido como carteira de hardware)", ""),
    (", vulgarmente chamado carteira de hardware,", ""),
    (", comumente chamado de hardware wallet,", ""),
    (", comumente chamado de carteira de hardware,", ""),
]


def main(folder: Path) -> None:
    for path in sorted(folder.glob("*.lang")):
        text = path.read_text()
        for old, new in WORDING:
            text = text.replace(old, new)
        path.write_text(text)
    seen = {}
    for path in sorted(folder.glob("*.lang")):
        current = None
        out = []
        changed = 0
        for line in path.read_text().splitlines(keepends=True):
            m = ENTRY_START.match(line)
            if m:
                current = m.group(1)
            elif not line.strip():
                current = None
            if current in IDS and "Talia Desktop" in line:
                seen.setdefault(current, set()).add(path.name)  # already done
            if current in IDS and "Liana" in line:
                new = rebrand(line)
                if new != line:
                    changed += 1
                    seen.setdefault(current, set()).add(path.name)
                line = new
            out.append(line)
        path.write_text("".join(out))
        print(f"{path.name}: {changed} lines")
    missing = IDS - set(seen)
    if missing:
        sys.exit(f"ids not found: {sorted(missing)}")


main(Path(sys.argv[1]))
