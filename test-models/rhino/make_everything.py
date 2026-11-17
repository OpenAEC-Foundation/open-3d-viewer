# -*- coding: utf-8 -*-
"""One Rhino 9 model with everything a normal file holds, saved three ways:
everything_meshes.3dm (as Rhino saves after showing it: render meshes included),
everything_small.3dm (no render meshes) and everything_v8.3dm (Rhino 8 format with meshes),
and everything.json: what Rhino says of every object."""
import os
import json
import math
import traceback
import System
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT")
TOL = sc.doc.ModelAbsoluteTolerance
log = open(os.path.join(OUT, "log_everything.txt"), "w")


def say(*a):
    log.write(" ".join(str(x) for x in a) + "\n")
    log.flush()


def color(r, g, b):
    return System.Drawing.Color.FromArgb(r, g, b)


def layer(name, c, parent=None, visible=True, locked=False):
    l = Rhino.DocObjects.Layer()
    l.Name = name
    l.Color = color(*c)
    if parent is not None:
        l.ParentLayerId = sc.doc.Layers[parent].Id
    idx = sc.doc.Layers.Add(l)
    lay = sc.doc.Layers[idx]
    if not visible:
        lay.IsVisible = False
    if locked:
        lay.IsLocked = True
    lay.CommitChanges()
    return idx


def attrs(lay, name, **kw):
    a = Rhino.DocObjects.ObjectAttributes()
    a.LayerIndex = lay
    a.Name = name
    for k, v in kw.items():
        setattr(a, k, v)
    return a


def material(name, rgb, transparency=0.0, pbr=False):
    m = Rhino.DocObjects.Material()
    m.Name = name
    m.DiffuseColor = color(*rgb)
    m.Transparency = transparency
    if pbr:
        try:
            m.ToPhysicallyBased()
            m.PhysicallyBased.BaseColor = Rhino.Display.Color4f(color(*rgb))
            m.PhysicallyBased.Opacity = 1.0 - transparency
        except Exception:
            say("pbr:", traceback.format_exc())
    return sc.doc.Materials.Add(m)


for i in [o.Id for o in sc.doc.Objects]:
    sc.doc.Objects.Delete(i, True)

L_WALLS = layer("Walls", (200, 40, 40))
L_INNER = layer("Interior", (40, 40, 200), parent=L_WALLS)
L_HIDDEN = layer("Hidden layer", (0, 160, 0), visible=False)
L_LOCKED = layer("Locked layer", (160, 120, 0), locked=True)
L_MAT = layer("Material layer", (90, 90, 90))
L_MISC = layer("Misc", (0, 0, 0))
M_RED = material("Basic red glass", (220, 30, 30), 0.5)
M_PBR = material("PBR blue", (30, 80, 220), 0.0, pbr=True)
lm = sc.doc.Layers[L_MAT]
lm.RenderMaterialIndex = material("Layer material green", (40, 180, 60))
lm.CommitChanges()

O = Rhino.DocObjects.ObjectColorSource
M = Rhino.DocObjects.ObjectMaterialSource


def box(x, y, z=0, s=2.0):
    return rg.Box(rg.BoundingBox(rg.Point3d(x, y, z), rg.Point3d(x + s, y + s, z + s))).ToBrep()


ids = []
ids.append(sc.doc.Objects.AddBrep(box(0, 0), attrs(L_WALLS, "Box by layer")))
ids.append(sc.doc.Objects.AddBrep(box(3, 0), attrs(L_INNER, "Box on sublayer")))
ids.append(sc.doc.Objects.AddBrep(box(6, 0), attrs(L_HIDDEN, "Box on hidden layer")))
ids.append(sc.doc.Objects.AddBrep(box(9, 0), attrs(L_LOCKED, "Box on locked layer")))
ids.append(sc.doc.Objects.AddBrep(box(12, 0), attrs(L_MISC, "Box object colour", ColorSource=O.ColorFromObject, ObjectColor=color(250, 200, 0))))
ids.append(sc.doc.Objects.AddBrep(box(15, 0), attrs(L_MISC, "Box basic material", MaterialSource=M.MaterialFromObject, MaterialIndex=M_RED)))
ids.append(sc.doc.Objects.AddBrep(box(18, 0), attrs(L_MISC, "Box PBR material", MaterialSource=M.MaterialFromObject, MaterialIndex=M_PBR)))
ids.append(sc.doc.Objects.AddBrep(box(21, 0), attrs(L_MAT, "Box layer material")))
hidden = attrs(L_MISC, "Hidden box")
hidden.Visible = False
ids.append(sc.doc.Objects.AddBrep(box(24, 0), hidden))
# per-face colours (Rhino 7 and later)
fb = box(27, 0)
try:
    for k, f in enumerate(fb.Faces):
        f.PerFaceColor = [color(255, 0, 0), color(0, 255, 0), color(0, 0, 255), color(255, 255, 0), color(0, 255, 255), color(255, 0, 255)][k]
except Exception:
    say("perface:", traceback.format_exc())
ids.append(sc.doc.Objects.AddBrep(fb, attrs(L_MISC, "Box face colours")))
# user text on the object and on its geometry
ut = attrs(L_MISC, "Box user text")
ut.SetUserString("Fire rating", "60 min")
gb = box(30, 0)
gb.SetUserString("Supplier", "ACME")
ids.append(sc.doc.Objects.AddBrep(gb, ut))
# extrusion, cylinder, sphere (surfaces), a mesh with vertex colours
ids.append(sc.doc.Objects.AddExtrusion(rg.Extrusion.Create(rg.ArcCurve(rg.Circle(rg.Point3d(1, 5, 0), 1)), 3, True), attrs(L_MISC, "Extruded cylinder")))
ids.append(sc.doc.Objects.AddSphere(rg.Sphere(rg.Point3d(5, 5, 1), 1), attrs(L_MISC, "Sphere")))
mesh = rg.Mesh.CreateFromSphere(rg.Sphere(rg.Point3d(9, 5, 1), 1), 24, 12)
mesh.VertexColors.Clear()
for v in mesh.Vertices:
    mesh.VertexColors.Add(color(int(127 + 127 * math.sin(v.X * 3)), int(127 + 127 * math.cos(v.Y * 3)), 128))
ids.append(sc.doc.Objects.AddMesh(mesh, attrs(L_MISC, "Coloured mesh")))
# a native SubD
try:
    subd = rg.SubD.CreateFromMesh(rg.Mesh.CreateFromBox(rg.Box(rg.BoundingBox(rg.Point3d(12, 4, 0), rg.Point3d(14, 6, 2))), 1, 1, 1))
    ids.append(sc.doc.Objects.AddSubD(subd, attrs(L_MISC, "SubD")))
except Exception:
    say("subd:", traceback.format_exc())
# points, a point cloud, curves, text, a hatch: not drawn as solids
ids.append(sc.doc.Objects.AddPoint(rg.Point3d(16, 5, 0), attrs(L_MISC, "Point")))
pc = rg.PointCloud([rg.Point3d(18 + 0.1 * i, 5 + 0.05 * (i % 7), 0.02 * i) for i in range(100)])
ids.append(sc.doc.Objects.AddPointCloud(pc, attrs(L_MISC, "Point cloud")))
ids.append(sc.doc.Objects.AddCurve(rg.Circle(rg.Point3d(22, 5, 0), 1).ToNurbsCurve(), attrs(L_MISC, "Circle curve")))
ids.append(sc.doc.Objects.AddText("Rhino 9", rg.Plane(rg.Point3d(24, 5, 0), rg.Vector3d.ZAxis), 0.5, "Arial", False, False, attrs(L_MISC, "Text")))
hatch = rg.Hatch.Create(rg.Rectangle3d(rg.Plane(rg.Point3d(26, 4, 0), rg.Vector3d.ZAxis), 2, 2).ToNurbsCurve(), 0, 0, 1, TOL)
if hatch:
    ids.append(sc.doc.Objects.AddHatch(hatch[0], attrs(L_MISC, "Hatch")))
# a block whose member takes its colour by parent, placed with an object colour
member = attrs(L_MISC, "")
member.ColorSource = O.ColorFromParent
idef = sc.doc.InstanceDefinitions.Add("Crate", "", rg.Point3d.Origin, [box(0, 0, 0, 1)], [member])
ids.append(sc.doc.Objects.AddInstanceObject(idef, rg.Transform.Translation(rg.Vector3d(0, 9, 0)), attrs(L_MISC, "Crate by parent", ColorSource=O.ColorFromObject, ObjectColor=color(0, 200, 200))))

# show it shaded, so that Rhino makes the render meshes, as after working in it
view = sc.doc.Views.ActiveView
DMD = Rhino.Display.DisplayModeDescription
mode = DMD.GetDisplayMode(DMD.ShadedId)
vp = view.ActiveViewport
vp.ChangeToPerspectiveProjection(True, 50.0)
vp.DisplayMode = mode
vp.ZoomExtents()
view.Redraw()
bmp = view.CaptureToBitmap(System.Drawing.Size(1400, 800), mode)
if bmp:
    bmp.Save(os.path.join(OUT, "everything.png"))
for o in sc.doc.Objects:
    try:
        o.CreateMeshes(Rhino.Geometry.MeshType.Render, Rhino.Geometry.MeshingParameters.Default, False)
    except Exception:
        pass

opts = Rhino.FileIO.FileWriteOptions()
opts.IncludeRenderMeshes = True
say("meshes", sc.doc.WriteFile(os.path.join(OUT, "everything_meshes.3dm"), opts))
opts = Rhino.FileIO.FileWriteOptions()
opts.IncludeRenderMeshes = False
say("small", sc.doc.WriteFile(os.path.join(OUT, "everything_small.3dm"), opts))
opts = Rhino.FileIO.FileWriteOptions()
opts.IncludeRenderMeshes = True
opts.FileVersion = 8
say("v8", sc.doc.WriteFile(os.path.join(OUT, "everything_v8.3dm"), opts))
opts = Rhino.FileIO.FileWriteOptions()
opts.IncludeRenderMeshes = True
opts.FileVersion = 7
say("v7", sc.doc.WriteFile(os.path.join(OUT, "everything_v7.3dm"), opts))

# what Rhino says of every object
rows = []
for o in sc.doc.Objects:
    a = o.Attributes
    lay = sc.doc.Layers[a.LayerIndex]
    mat = o.GetMaterial(True)
    row = {
        "name": a.Name or "",
        "type": str(o.ObjectType),
        "layer": lay.FullPath,
        "draw_color": [int(o.Attributes.DrawColor(sc.doc).R), int(o.Attributes.DrawColor(sc.doc).G), int(o.Attributes.DrawColor(sc.doc).B)],
        "material": mat.Name if mat else None,
        "material_diffuse": [int(mat.DiffuseColor.R), int(mat.DiffuseColor.G), int(mat.DiffuseColor.B)] if mat else None,
        "pbr": bool(mat.IsPhysicallyBased) if mat else None,
        "material_transparency": float(mat.Transparency) if mat else None,
        "visible": bool(a.Visible and lay.IsVisible),
        "user": dict((str(k), str(a.GetUserString(k))) for k in a.GetUserStrings().AllKeys) if a.GetUserStrings() else {},
    }
    g = o.Geometry
    try:
        if isinstance(g, rg.Extrusion):
            g = g.ToBrep()
        if isinstance(g, (rg.Brep, rg.Mesh)):
            amp = rg.AreaMassProperties.Compute(g)
            row["area"] = float(amp.Area) if amp else None
    except Exception:
        pass
    rows.append(row)
try:
    text = json.dumps({"units": str(sc.doc.ModelUnitSystem), "objects": rows}, indent=1)
    open(os.path.join(OUT, "everything.json"), "w").write(text)
except Exception:
    say(traceback.format_exc())
sc.doc.Modified = False
say("done")
log.close()
