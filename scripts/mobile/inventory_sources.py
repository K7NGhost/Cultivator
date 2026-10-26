"""Inspect LEAPP source catalogs without importing optional parser dependencies."""
import ast
import json
from pathlib import Path
import sys


def inspect(root):
    root = Path(root)
    count = {"v1": 0, "v2": 0, "modules": 0, "alternate_modules": 0}
    results = []
    for folder in ("artifacts", "alternate_artifacts"):
        for path in sorted((root / "scripts" / folder).rglob("*.py")):
            if path.name == "__init__.py":
                continue
            tree = ast.parse(path.read_text(encoding="utf-8-sig"))
            defs = []
            for node in tree.body:
                if isinstance(node, ast.Assign) and isinstance(node.value, ast.Dict):
                    names = [n.id for n in node.targets if isinstance(n, ast.Name)]
                    for version, variable in (("v1", "__artifacts__"), ("v2", "__artifacts_v2__")):
                        if variable in names:
                            for key, value in zip(node.value.keys, node.value.values):
                                try:
                                    key = ast.literal_eval(key)
                                except (ValueError, TypeError):
                                    key = ast.unparse(key)
                                info = {"key": key, "version": version}
                                if isinstance(value, ast.Dict):
                                    for field, field_value in zip(value.keys, value.values):
                                        try:
                                            field = ast.literal_eval(field)
                                            if field in ("name", "category", "paths", "function", "description"):
                                                info[field] = ast.literal_eval(field_value)
                                        except (ValueError, TypeError):
                                            pass
                                defs.append(info)
                                count[version] += 1
            results.append({"path": path.relative_to(root).as_posix(), "artifacts": defs})
            count["alternate_modules" if folder == "alternate_artifacts" else "modules"] += 1
    groups = {}
    for name in ("scripts", "leapp_functions", "assets", "bin", "whl_files"):
        files = [p for p in (root / name).rglob("*") if p.is_file() and "__pycache__" not in p.parts]
        groups[name] = {"files": len(files), "bytes": sum(p.stat().st_size for p in files)}
    return {"root": str(root), "counts": count, "groups": groups, "modules": results}


if __name__ == "__main__":
    for source in sys.argv[1:]:
        info = inspect(source)
        print(json.dumps({k: v for k, v in info.items() if k != "modules"}, indent=2))
