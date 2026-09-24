import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) < 4:
        raise SystemExit(
            "usage: altium_monkey_render.py <symbol|symbol_parts|footprint> <library> <name> [part_id] [output]"
        )

    kind, library, name = sys.argv[1:4]
    if kind == "symbol":
        from altium_monkey.altium_schlib import AltiumSchLib

        part_id = int(sys.argv[4]) if len(sys.argv) == 6 else None
        output = sys.argv[5] if len(sys.argv) == 6 else sys.argv[4]
        svg = AltiumSchLib(Path(library)).symbol_to_svg(name, part_id=part_id)
    elif kind == "symbol_parts":
        from altium_monkey.altium_schlib import AltiumSchLib

        symbol = next(
            (item for item in AltiumSchLib(Path(library)).symbols if item.name == name),
            None,
        )
        if symbol is None:
            raise RuntimeError(f"Symbol '{name}' was not found")
        print(symbol.part_count)
        return
    elif kind == "footprint":
        from altium_monkey.altium_pcblib import AltiumPcbLib

        output = sys.argv[4]
        lib = AltiumPcbLib(Path(library))
        footprint = next((item for item in lib.footprints if item.name == name), None)
        if footprint is None:
            raise RuntimeError(f"Footprint '{name}' was not found")
        svg = footprint.to_svg()
    else:
        raise RuntimeError(f"Unknown render kind: {kind}")

    Path(output).write_text(svg, encoding="utf-8")


if __name__ == "__main__":
    main()
