import zipfile, re, sys, html
def read(path):
    z = zipfile.ZipFile(path)
    ss = [html.unescape("".join(re.findall(r"<t[^>]*>([^<]*)</t>", si))) for si in re.findall(r"<si>(.*?)</si>", z.read("xl/sharedStrings.xml").decode(), re.S)]
    wb = z.read("xl/workbook.xml").decode()
    rels = dict(re.findall(r'<Relationship Id="([^"]+)"[^>]*Target="([^"]+)"', z.read("xl/_rels/workbook.xml.rels").decode()))
    rels.update({b: a for a, b in []})
    out = {}
    for m in re.finditer(r'<sheet [^>]*name="([^"]+)"[^>]*r:id="([^"]+)"', wb):
        name, rid = m.groups()
        tgt = rels.get(rid) or dict((b, a) for a, b in re.findall(r'Target="([^"]+)"[^>]*Id="([^"]+)"', z.read("xl/_rels/workbook.xml.rels").decode())).get(rid)
        x = z.read("xl/" + tgt).decode()
        rows = []
        for r in re.findall(r"<row[^>]*>(.*?)</row>", x, re.S):
            cells = {}
            for c in re.finditer(r'<c r="([A-Z]+)\d+"([^>]*?)(?:/>|>(.*?)</c>)', r, re.S):
                col, attrs, body = c.groups()
                v = re.search(r"<v>([^<]*)</v>", body or "")
                if not v: continue
                cells[col] = ss[int(v.group(1))] if 't="s"' in attrs else html.unescape(v.group(1))
            rows.append(cells)
        out[name] = rows
    return out
if __name__ == "__main__":
    d = read(sys.argv[1])
    for name, rows in d.items():
        print("==", name, len(rows))
        for r in rows[: int(sys.argv[2]) if len(sys.argv) > 2 else 3]:
            print("  ", {k: v[:40] for k, v in r.items()})
