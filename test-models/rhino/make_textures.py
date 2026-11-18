# -*- coding: utf-8 -*-
"""Textures (textures.3dm, textures_embedded.3dm): a box and a sphere with a bitmap texture
(checker.png, written next to the model), a mesh with its own texture coordinates, a box with a
physically based material whose base colour is the texture, and a box with a planar decal
(decal.png). textures_embedded.3dm has the images in the file. textures.json holds what Rhino
reads back: per object its render meshes' vertices and texture coordinates; textures.png is
Rhino's rendered view of the named view "Compare"."""
import os
import json
import System
import System.Drawing as sd
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT")
log = []


def checker(path):
    """256 x 256, 8 x 8 cells black and white; the top left cell red, the top right green and
    the bottom left blue, so that the orientation shows."""
    bmp = sd.Bitmap(256, 256)
    g = sd.Graphics.FromImage(bmp)
    for cy in range(8):
        for cx in range(8):
            c = sd.Color.White if (cx + cy) % 2 == 0 else sd.Color.Black
            if (cx, cy) == (0, 0):
                c = sd.Color.FromArgb(220, 30, 30)
            elif (cx, cy) == (7, 0):
                c = sd.Color.FromArgb(30, 180, 30)
            elif (cx, cy) == (0, 7):
                c = sd.Color.FromArgb(30, 60, 220)
            g.FillRectangle(sd.SolidBrush(c), cx * 32, cy * 32, 32, 32)
    g.Dispose()
    bmp.Save(path, sd.Imaging.ImageFormat.Png)


def decal_image(path):
    """128 x 128: yellow with a blue L along its left and bottom edges."""
    bmp = sd.Bitmap(128, 128)
    g = sd.Graphics.FromImage(bmp)
    g.FillRectangle(sd.SolidBrush(sd.Color.FromArgb(240, 200, 20)), 0, 0, 128, 128)
    blue = sd.SolidBrush(sd.Color.FromArgb(20, 40, 200))
    g.FillRectangle(blue, 0, 0, 24, 128)
    g.FillRectangle(blue, 0, 104, 128, 24)
    g.Dispose()
    bmp.Save(path, sd.Imaging.ImageFormat.Png)


def add_material(doc, name, path, pbr):
    m = Rhino.DocObjects.Material()
    m.Name = name
    if pbr:
        m.ToPhysicallyBased()
        m.PhysicallyBased.BaseColor = Rhino.Display.Color4f(0.8, 0.8, 0.8, 1.0)
        t = Rhino.DocObjects.Texture()
        t.FileName = path
        m.SetTexture(t, Rhino.DocObjects.TextureType.PBR_BaseColor)
    else:
        m.DiffuseColor = sd.Color.FromArgb(200, 200, 200)
        m.SetBitmapTexture(path)
    return doc.Materials.Add(m)


def attrs(material, name):
    a = Rhino.DocObjects.ObjectAttributes()
    a.Name = name
    if material >= 0:
        a.MaterialSource = Rhino.DocObjects.ObjectMaterialSource.MaterialFromObject
        a.MaterialIndex = material
    return a


def bitmap_texture(doc, path):
    t = Rhino.Render.RenderContent.Create(Rhino.Render.ContentUuids.BitmapTextureType, getattr(Rhino.Render.RenderContent.ShowContentChooserFlags, "None"), doc)
    t.BeginChange(Rhino.Render.RenderContent.ChangeContexts.Program)
    t.SetParameter("filename", path)
    t.EndChange()
    doc.RenderTextures.Add(t)
    return t


def add_decal(doc, oid, texture):
    p = Rhino.Render.DecalCreateParams()
    p.TextureInstanceId = texture.Id
    p.DecalMapping = Rhino.Render.DecalMapping.Planar
    p.DecalProjection = Rhino.Render.DecalProjection.Both
    p.Origin = rg.Point3d(6250, 0, 250)
    p.VectorUp = rg.Vector3d(0, 0, 500)
    p.VectorAcross = rg.Vector3d(500, 0, 0)
    p.Transparency = 0.0
    p.MapToInside = False
    d = Rhino.Render.Decal.Create(p)
    o = doc.Objects.FindId(oid)
    a = o.Attributes.Duplicate()
    a.Decals.Add(d)
    doc.Objects.ModifyAttributes(oid, a, True)


def render_meshes(o):
    meshes = []
    try:
        meshes = [m for m in o.GetMeshes(Rhino.Geometry.MeshType.Render) if m is not None]
    except Exception:
        pass
    if not meshes and isinstance(o.Geometry, rg.Mesh):
        meshes = [o.Geometry]
    out = []
    for m in meshes:
        out.append({
            "vertices": [[v.X, v.Y, v.Z] for v in m.Vertices],
            "uv": [[t.X, t.Y] for t in m.TextureCoordinates],
        })
    return out


def main():
    doc = sc.doc
    for i in [o.Id for o in doc.Objects]:
        doc.Objects.Delete(i, True)
    doc.AdjustModelUnitSystem(Rhino.UnitSystem.Millimeters, False)
    tex = os.path.join(OUT, "checker.png")
    checker(tex)
    dec = os.path.join(OUT, "decal.png")
    decal_image(dec)
    basic = add_material(doc, "Checker", tex, False)
    pbr = add_material(doc, "Checker PBR", tex, True)
    grey = Rhino.DocObjects.Material()
    grey.Name = "Grey"
    grey.DiffuseColor = sd.Color.FromArgb(160, 160, 160)
    plain = doc.Materials.Add(grey)

    box = rg.Box(rg.Plane.WorldXY, rg.Interval(0, 1000), rg.Interval(0, 1000), rg.Interval(0, 1000))
    doc.Objects.AddBrep(box.ToBrep(), attrs(basic, "Checker box"))
    doc.Objects.AddSphere(rg.Sphere(rg.Point3d(2000, 500, 500), 500), attrs(basic, "Checker sphere"))
    # a vertical square mesh in front (y = 0) with its own coordinates: the texture twice across
    mesh = rg.Mesh()
    for x, z, u, v in [(3000, 0, 0, 0), (4000, 0, 2, 0), (4000, 1000, 2, 1), (3000, 1000, 0, 1)]:
        mesh.Vertices.Add(x, 0, z)
        mesh.TextureCoordinates.Add(u, v)
    mesh.Faces.AddFace(0, 1, 2, 3)
    mesh.Normals.ComputeNormals()
    doc.Objects.AddMesh(mesh, attrs(basic, "Mesh with coordinates"))
    box2 = rg.Box(rg.Plane.WorldXY, rg.Interval(4500, 5500), rg.Interval(0, 1000), rg.Interval(0, 1000))
    doc.Objects.AddBrep(box2.ToBrep(), attrs(pbr, "PBR box"))
    box3 = rg.Box(rg.Plane.WorldXY, rg.Interval(6000, 7000), rg.Interval(0, 1000), rg.Interval(0, 1000))
    did = doc.Objects.AddBrep(box3.ToBrep(), attrs(plain, "Decal box"))
    try:
        add_decal(doc, did, bitmap_texture(doc, dec))
    except Exception as e:
        log.append("decal: %s" % e)

    # the view to compare: rendered, from the front left above
    view = doc.Views.ActiveView
    vp = view.ActiveViewport
    vp.ChangeToPerspectiveProjection(True, 50)
    vp.SetCameraLocations(rg.Point3d(3500, 500, 500), rg.Point3d(-1500, -7500, 4500))
    vp.ZoomExtents()
    for mode in Rhino.Display.DisplayModeDescription.GetDisplayModes():
        if mode.EnglishName == "Rendered":
            vp.DisplayMode = mode
    doc.NamedViews.Add("Compare", vp.Id)
    view.Redraw()

    o = Rhino.FileIO.FileWriteOptions()
    doc.WriteFile(os.path.join(OUT, "textures.3dm"), o)
    o2 = Rhino.FileIO.FileWriteOptions()
    try:
        o2.IncludeBitmapTable = True
    except Exception as e:
        log.append("bitmaps: %s" % e)
    doc.WriteFile(os.path.join(OUT, "textures_embedded.3dm"), o2)
    view.Redraw()
    try:
        bmp = view.CaptureToBitmap(sd.Size(1200, 700))
        bmp.Save(os.path.join(OUT, "textures.png"), sd.Imaging.ImageFormat.Png)
    except Exception as e:
        log.append("capture: %s" % e)

    out = {"objects": [], "camera": {}, "log": log}
    cam = view.ActiveViewport
    out["camera"] = {"location": [cam.CameraLocation.X, cam.CameraLocation.Y, cam.CameraLocation.Z],
                     "target": [cam.CameraTarget.X, cam.CameraTarget.Y, cam.CameraTarget.Z]}
    for ob in doc.Objects:
        out["objects"].append({"name": ob.Attributes.Name, "meshes": render_meshes(ob),
                               "decals": len(list(ob.Attributes.Decals))})
    f = Rhino.FileIO.File3dm.Read(os.path.join(OUT, "textures.3dm"))
    out["materials"] = []
    for m in f.AllMaterials:
        out["materials"].append({"name": m.Name, "textures": [t.FileName for t in m.GetTextures()]})
    out["embedded"] = [e.Filename for e in f.EmbeddedFiles] if hasattr(f, "EmbeddedFiles") else "n/a"
    with open(os.path.join(OUT, "textures.json"), "w") as fh:
        json.dump(out, fh, indent=1)


try:
    main()
except Exception:
    import traceback
    with open(os.path.join(OUT, "textures_error.txt"), "w") as fh:
        fh.write(traceback.format_exc() + "\n" + "\n".join(log))
