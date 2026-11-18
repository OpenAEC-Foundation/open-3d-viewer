# -*- coding: utf-8 -*-
"""Lights (lights.3dm): a box and one light of each kind (point, spot, directional, linear,
rectangular, ambient and a camera directional one), each with its own colour, intensity and
place; one switched off, one hidden and one on a hidden layer. With what Rhino reads of them
(lights.json)."""Lights (lights.3dm): a box and one light of each kind (point, spot, directional, linear,
rectangular, ambient and a camera directional one), each with its own colour, intensity and
place, one switched off, one hidden and one on a hidden layer; with what Rhino reads of them (lights.json)."""
import os
import json
import System
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT")


def light(name, style, location, direction, diffuse, intensity):
    l = rg.Light()
    l.LightStyle = style
    l.Location = location
    l.Direction = direction
    l.Diffuse = diffuse
    l.Intensity = intensity
    l.Name = name
    return l


def vec(v):
    return [v.X, v.Y, v.Z]


def color(c):
    return [int(c.R), int(c.G), int(c.B), 255 - int(c.A)]


def main():
    doc = sc.doc
    for i in [o.Id for o in doc.Objects]:
        doc.Objects.Delete(i, True)
    box = rg.Box(rg.Plane.WorldXY, rg.Interval(0, 10), rg.Interval(0, 6), rg.Interval(0, 3))
    doc.Objects.AddBrep(box.ToBrep())
    hidden = doc.Layers.Add("Hidden lights", System.Drawing.Color.Gray)
    doc.Layers[hidden].IsVisible = False
    C = System.Drawing.Color.FromArgb
    S = rg.LightStyle
    lights = []
    p = light("Point", S.WorldPoint, rg.Point3d(1, 2, 8), rg.Vector3d(0, 0, -1), C(255, 200, 150), 0.8)
    p.AttenuationType = rg.Light.Attenuation.InverseSquared
    p.ShadowIntensity = 0.4
    p.PowerWatts = 60.0
    lights.append((p, None))
    s = light("Spot", S.WorldSpot, rg.Point3d(12, -4, 9), rg.Vector3d(-6, 5, -8), C(120, 220, 255), 0.65)
    s.SpotAngleRadians = 0.5
    s.HotSpot = 0.35
    s.Ambient = C(10, 20, 30)
    s.Specular = C(250, 240, 230)
    lights.append((s, None))
    d = light("Directional", S.WorldDirectional, rg.Point3d(-5, -5, 10), rg.Vector3d(1, 2, -3), C(255, 255, 200), 0.9)
    lights.append((d, "hidden object"))
    ln = light("Linear", S.WorldLinear, rg.Point3d(2, 1, 5), rg.Vector3d(0, 0, -2), C(200, 255, 200), 0.55)
    ln.Length = rg.Vector3d(6, 0, 0)
    ln.AttenuationType = rg.Light.Attenuation.Linear
    lights.append((ln, None))
    r = light("Rectangular", S.WorldRectangular, rg.Point3d(3, 2, 6), rg.Vector3d(0, 0, -1.5), C(255, 180, 220), 0.7)
    r.Length = rg.Vector3d(4, 0, 0)
    r.Width = rg.Vector3d(0, 2.5, 0)
    r.IsEnabled = False
    lights.append((r, None))
    a = light("Ambient", S.Ambient, rg.Point3d(0, 0, 0), rg.Vector3d(0, 0, -1), C(40, 50, 60), 0.3)
    lights.append((a, hidden))
    cam = light("Headlight", S.CameraDirectional, rg.Point3d(0, 0, 0), rg.Vector3d(0, 0, -1), C(230, 230, 255), 0.45)
    lights.append((cam, None))
    added = {}
    for l, layer in lights:
        attributes = Rhino.DocObjects.ObjectAttributes()
        attributes.Name = l.Name
        if layer == "hidden object":
            attributes.Visible = False
        elif layer is not None:
            attributes.LayerIndex = layer
        index = doc.Lights.Add(l, attributes)
        added[l.Name] = index
    doc.WriteFile(os.path.join(OUT, "lights.3dm"), Rhino.FileIO.FileWriteOptions())
    f = Rhino.FileIO.File3dm.Read(os.path.join(OUT, "lights.3dm"))
    out = {"added": added, "lights": [], "objects": []}

    def record(g, attributes):
        out["lights"].append({
            "id": str(g.Id),
            "name": g.Name,
            "attributes_id": str(attributes.ObjectId),
            "attributes_name": attributes.Name,
            "layer": attributes.LayerIndex,
            "visible": attributes.Visible,
            "enabled": g.IsEnabled,
            "style": int(g.LightStyle),
            "location": vec(g.Location),
            "direction": vec(g.Direction),
            "length": vec(g.Length),
            "width": vec(g.Width),
            "ambient": color(g.Ambient),
            "diffuse": color(g.Diffuse),
            "specular": color(g.Specular),
            "intensity": g.Intensity,
            "watts": g.PowerWatts,
            "spot_angle": g.SpotAngleRadians,
            "spot_exponent": g.SpotExponent,
            "hotspot": g.HotSpot,
            "attenuation": vec(g.AttenuationVector),
            "shadow_intensity": g.ShadowIntensity,
        })

    # File3dm keeps the lights with the objects
    for ob in f.Objects:
        if isinstance(ob.Geometry, rg.Light):
            record(ob.Geometry, ob.Attributes)
        out["objects"].append(str(ob.Geometry.ObjectType))
    with open(os.path.join(OUT, "lights.json"), "w") as fh:
        json.dump(out, fh, indent=1)


try:
    main()
except Exception:
    import traceback
    with open(os.path.join(OUT, "lights_error.txt"), "w") as fh:
        fh.write(traceback.format_exc())
