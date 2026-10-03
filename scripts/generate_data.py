#!/usr/bin/env python3
"""Generate command data from a vanilla Minecraft server jar.

The file holds the full command tree and the registry entries that commands
accept as arguments, which drive completion.

Usage: scripts/generate_data.py path/to/server.jar [output.json]

Without an output path this refreshes the copy embedded in the binary. With
one, the result is for --data-file.

Requires a `java` new enough to run that server version.
"""
import json
import os
import subprocess
import sys
import tempfile
import zipfile

# Registries for arguments whose parser doesn't name its registry.
EXTRA_REGISTRIES = [
    "minecraft:item",
    "minecraft:block",
    "minecraft:particle_type",
    "minecraft:sound_event",
    "minecraft:loot_table",
    "minecraft:dialog",
    "minecraft:worldgen/configured_feature",
    "minecraft:worldgen/feature",
]
# Too big / too niche to be worth embedding.
SKIPPED_REGISTRIES = {"minecraft:test_instance", "minecraft:worldgen/template_pool"}


def strip(node):
    """Drop the fields the completer doesn't use."""
    node.pop("permissions", None)
    for child in node.get("children", {}).values():
        strip(child)
    return node


def wanted_registries(node, found):
    registry = (node.get("properties") or {}).get("registry")
    if registry:
        found.add(registry)
    for child in node.get("children", {}).values():
        wanted_registries(child, found)
    return found


def short(name):
    return name[len("minecraft:"):] if name.startswith("minecraft:") else name


def main():
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__)
    jar = os.path.abspath(sys.argv[1])
    out_path = os.path.join(os.path.dirname(__file__), "..", "data", "minecraft.json")
    if len(sys.argv) == 3:
        out_path = sys.argv[2]

    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(
            ["java", "-DbundlerMainClass=net.minecraft.data.Main", "-jar", jar,
             "--reports", "--output", "out"],
            cwd=tmp, check=True, stdout=subprocess.DEVNULL,
        )
        reports = os.path.join(tmp, "out", "reports")
        with open(os.path.join(reports, "commands.json")) as f:
            commands = strip(json.load(f))
        with open(os.path.join(reports, "registries.json")) as f:
            builtin = json.load(f)

        # Data-driven registries (biomes, enchantments, ...) only exist as
        # files inside the jar. Older jars aren't bundles, so try both.
        with zipfile.ZipFile(jar) as outer:
            version = json.loads(outer.read("version.json"))["id"]
        inner = os.path.join(tmp, "versions", version, f"server-{version}.jar")
        with zipfile.ZipFile(inner if os.path.exists(inner) else jar) as z:
            data_files = [n for n in z.namelist()
                          if n.startswith("data/minecraft/") and n.endswith(".json")]

    registries = {}
    wanted = wanted_registries(commands, set()) | set(EXTRA_REGISTRIES)
    for registry in sorted(wanted - SKIPPED_REGISTRIES):
        if registry in builtin:
            entries = builtin[registry]["entries"]
        else:
            prefix = f"data/minecraft/{short(registry)}/"
            entries = [n[len(prefix):-len(".json")] for n in data_files if n.startswith(prefix)]
        if entries:
            registries[short(registry)] = sorted(short(e) for e in entries)

    with open(out_path, "w") as f:
        json.dump({"version": version, "commands": commands, "registries": registries},
                  f, separators=(",", ":"), sort_keys=True)
        f.write("\n")
    print(f"Wrote {os.path.normpath(out_path)} for Minecraft {version}: "
          f"{len(commands['children'])} commands, "
          f"{sum(map(len, registries.values()))} registry entries")


if __name__ == "__main__":
    main()
