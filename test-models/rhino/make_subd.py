# -*- coding: utf-8 -*-
"""SubD test models (subd.3dm) and the area of each SubD's limit surface as Rhino computes it
from its exact NURBS form (subd.json)."""
import os
import json
import math
import traceback
import System
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT")
log = open(os.path.join(OUT, "log_subd.txt"), "w")


def say(*a):
    log.write(" ".join(str(x) for x in a) + "\n")
    log.flush()


for i in [o.Id for o in sc.doc.Objects]:
    sc.doc.Objects.Delete(i, True)
lay = sc.doc.Layers.Add("SubD", System.Drawing.Color.FromArgb(200, 120, 160))


def add(subd, name):
    if subd is None:
        say("None:", name)
        return
    a = Rhino.DocObjects.ObjectAttributes()
    a.LayerIndex = lay
    a.Name = name
    sc.doc.Objects.AddSubD(subd, a)


def box_mesh(x, y, z, sx, sy, sz, n=1):
    return rg.Mesh.CreateFromBox(rg.Box(rg.BoundingBox(rg.Point3d(x, y, z), rg.Point3d(x + sx, y + sy, z + sz))), n, n, n)


try:
    # a smooth box
    add(rg.SubD.CreateFromMesh(box_mesh(0, 0, 0, 4, 3, 2)), "Smooth box")
    # a box with creased (interior) edges: the top edges creased
    s = rg.SubD.CreateFromMesh(box_mesh(6, 0, 0, 4, 3, 2))
    top = [e for e in s.Edges if abs(e.VertexFrom.ControlNetPoint.Z - 2) < 1e-9 and abs(e.VertexTo.ControlNetPoint.Z - 2) < 1e-9]
    for e in top:
        e.Tag = rg.SubDEdgeTag.Crease
    s.UpdateAllTagsAndSectorCoefficients()
    add(s, "Creased top")
    # an open SubD (a plane of 3 by 3 faces, bumped) whose boundary is creased, with corners
    m = rg.Mesh.CreateFromPlane(rg.Plane(rg.Point3d(15, 1.5, 0), rg.Vector3d.ZAxis), rg.Interval(-2, 2), rg.Interval(-2, 2), 3, 3)
    for k in range(m.Vertices.Count):
        p = m.Vertices[k]
        if abs(p.X - 15) < 1.0 and abs(p.Y - 1.5) < 1.0:
            m.Vertices.SetVertex(k, p.X, p.Y, 1.5)
    opts = rg.SubDCreationOptions()
    opts.InteriorCreaseTest = rg.SubDCreationOptions.InteriorCreaseOption.None
    try:
        opts.ConvexCornerTest = rg.SubDCreationOptions.ConvexCornerOption.AtMeshCorner
    except Exception:
        pass
    add(rg.SubD.CreateFromMesh(m, opts), "Open patch")
    # triangles and a pentagon: an icosahedron-like ball and a pentagonal prism
    ico = rg.Mesh.CreateIcoSphere(rg.Sphere(rg.Point3d(22, 1.5, 1.5), 1.5), 1)
    add(rg.SubD.CreateFromMesh(ico), "Triangle ball")
    pent = rg.Mesh()
    for k in range(5):
        a = 2 * math.pi * k / 5
        pent.Vertices.Add(27 + 1.5 * math.cos(a), 1.5 + 1.5 * math.sin(a), 0)
    for k in range(5):
        a = 2 * math.pi * k / 5
        pent.Vertices.Add(27 + 1.5 * math.cos(a), 1.5 + 1.5 * math.sin(a), 3)
    pent.Ngons.AddNgon(rg.MeshNgon.Create([4, 3, 2, 1, 0], []))
    for k in range(5):
        pent.Faces.AddFace(k, (k + 1) % 5, 5 + (k + 1) % 5, 5 + k)
    # caps as ngons
    f0 = pent.Faces.Count
    pent.Faces.AddFace(0, 4, 3, 2)
    pent.Faces.AddFace(0, 2, 1)
    f1 = pent.Faces.Count
    pent.Faces.AddFace(5, 6, 7, 8)
    pent.Faces.AddFace(5, 8, 9)
    pent.Ngons.Clear()
    pent.Ngons.AddNgon(rg.MeshNgon.Create([0, 4, 3, 2, 1], [f0, f0 + 1]))
    pent.Ngons.AddNgon(rg.MeshNgon.Create([5, 6, 7, 8, 9], [f1, f1 + 1]))
    pent.Normals.ComputeNormals()
    add(rg.SubD.CreateFromMesh(pent), "Pentagon prism")
    # semi-sharp edges (Rhino 8 and later): the vertical edges of a box, constant and varying
    def sharpness(s0, s1):
        for make in (lambda: rg.SubDEdgeSharpness(s0, s1), lambda: rg.SubDEdgeSharpness.FromIntervalPercentage(s0 / 4.0, s1 / 4.0)):
            try:
                return make()
            except Exception:
                pass
        return None
    for name, x, (s0, s1) in (("Sharp box", 31, (2.0, 2.0)), ("Varying sharp box", 35, (1.0, 3.0))):
        s = rg.SubD.CreateFromMesh(box_mesh(x, 0, 0, 3, 3, 3))
        done = 0
        for e in list(s.Edges):
            a, b = e.VertexFrom.ControlNetPoint, e.VertexTo.ControlNetPoint
            if abs(a.X - b.X) < 1e-9 and abs(a.Y - b.Y) < 1e-9:
                sh = sharpness(s0, s1) if a.Z < b.Z else sharpness(s1, s0)
                try:
                    from System.Collections.Generic import List
                    lst = List[rg.SubDEdge]()
                    lst.Add(e)
                    if s.SetEdgeSharpness(lst, sh, False) > 0:
                        done += 1
                        continue
                except Exception as ex:
                    say("SetEdgeSharpness:", ex)
                try:
                    e.Sharpness = sh
                    done += 1
                except Exception as ex:
                    say("Sharpness=", ex)
        say(name, "sharp edges set:", done, "count:", s.SharpEdgeCount() if callable(getattr(s, "SharpEdgeCount", None)) else "?")
        add(s, name)
    # a dart: one interior crease edge that ends inside the surface
    s = rg.SubD.CreateFromMesh(box_mesh(40, 0, 0, 3, 3, 3, 2))
    try:
        best = None
        for e in s.Edges:
            mid = 0.5 * (e.VertexFrom.ControlNetPoint + e.VertexTo.ControlNetPoint)
            if abs(mid.Z - 3) < 1e-9 and abs(mid.X - 41.5) < 1e-9 and mid.Y > 1.4 and mid.Y < 1.6:
                best = e
        if best is None:
            for e in s.Edges:
                a, b = e.VertexFrom.ControlNetPoint, e.VertexTo.ControlNetPoint
                if abs(a.Z - 3) < 1e-9 and abs(b.Z - 3) < 1e-9 and (abs(a.X - 41.5) < 1e-9 or abs(b.X - 41.5) < 1e-9) and (abs(a.Y - 1.5) < 1e-9 or abs(b.Y - 1.5) < 1e-9):
                    best = e
                    break
        if best is not None:
            best.Tag = rg.SubDEdgeTag.Crease
        s.UpdateAllTagsAndSectorCoefficients()
    except Exception:
        say("dart:", traceback.format_exc())
    add(s, "Dart")
    # a finer SubD: a torus-like ring from a quad mesh
    ring = rg.Mesh.CreateFromSurface(rg.Torus(rg.Plane(rg.Point3d(49, 1.5, 1), rg.Vector3d.ZAxis), 3, 1).ToNurbsSurface(), rg.MeshingParameters.Coarse)
    add(rg.SubD.CreateFromMesh(ring), "Ring")
except Exception:
    say(traceback.format_exc())

opts = Rhino.FileIO.FileWriteOptions()
opts.IncludeRenderMeshes = True
say("save", sc.doc.WriteFile(os.path.join(OUT, "subd.3dm"), opts))
view = sc.doc.Views.ActiveView
if view:
    vp = view.ActiveViewport
    vp.ChangeToPerspectiveProjection(True, 50.0)
    DMD = Rhino.Display.DisplayModeDescription
    mode = DMD.GetDisplayMode(DMD.ShadedId)
    vp.DisplayMode = mode
    vp.ZoomExtents()
    view.Redraw()
    bmp = view.CaptureToBitmap(System.Drawing.Size(1400, 700), mode)
    if bmp:
        bmp.Save(os.path.join(OUT, "subd.png"))

rows = []
for o in sc.doc.Objects:
    g = o.Geometry
    if isinstance(g, rg.SubD):
        area = None
        try:
            b = g.ToBrep(rg.SubDToBrepOptions.Default)
            amp = rg.AreaMassProperties.Compute(b)
            area = float(amp.Area) if amp else None
        except Exception:
            say("area:", traceback.format_exc())
        rows.append({"name": str(o.Attributes.Name), "area": area, "faces": int(g.Faces.Count)})
open(os.path.join(OUT, "subd.json"), "w").write(json.dumps(rows, indent=1))
sc.doc.Modified = False
say("done")
log.close()
