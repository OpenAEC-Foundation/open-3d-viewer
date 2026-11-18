# -*- coding: utf-8 -*-
"""Document settings (settings.3dm): an earth anchor point, texts in different fonts (Arial bold,
Times New Roman italic, a style in Courier New), a dimension style in Times New Roman italic with
a dimension, two named construction planes, the Top view's construction plane moved, two named
layer states, a snapshot, an object extruded with history, document user text and custom render
mesh settings; with what Rhino reads of them (settings.json)."""
import os
import json
import System
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT")
C = System.Drawing.Color.FromArgb


def xyz(p):
    return [p.X, p.Y, p.Z]


def color(c):
    return [int(c.R), int(c.G), int(c.B), 255 - int(c.A)]


def plane(p):
    return {"origin": xyz(p.Origin), "x": xyz(p.XAxis), "y": xyz(p.YAxis)}


def font(f):
    return {"family": f.FamilyName, "face": f.FaceName, "quartet": f.QuartetName, "postscript": f.PostScriptName,
            "logfont": f.LogfontName, "bold": f.Bold, "italic": f.Italic, "weight": int(f.Weight), "style": int(f.Style),
            "underlined": f.Underlined, "strikeout": f.Strikeout}


def cplane(c):
    return {"name": c.Name, "plane": plane(c.Plane), "grid_spacing": c.GridSpacing, "snap_spacing": c.SnapSpacing,
            "grid_line_count": c.GridLineCount, "thick_frequency": c.ThickLineFrequency, "depth_buffered": c.DepthBuffered,
            "show_grid": c.ShowGrid, "show_axes": c.ShowAxes, "show_z_axis": c.ShowZAxis}


def meshing(m):
    return {"texture_range": int(m.TextureRange), "jagged_seams": m.JaggedSeams, "refine_grid": m.RefineGrid,
            "double_precision": m.DoublePrecision, "simple_planes": m.SimplePlanes, "compute_curvature": m.ComputeCurvature,
            "closed_object_post_process": m.ClosedObjectPostProcess, "grid_min_count": m.GridMinCount,
            "grid_max_count": m.GridMaxCount, "grid_angle": m.GridAngle, "grid_aspect_ratio": m.GridAspectRatio,
            "grid_amplification": m.GridAmplification, "tolerance": m.Tolerance, "minimum_tolerance": m.MinimumTolerance,
            "relative_tolerance": m.RelativeTolerance, "minimum_edge_length": m.MinimumEdgeLength,
            "maximum_edge_length": m.MaximumEdgeLength, "refine_angle": m.RefineAngle}


def run(script):
    return Rhino.RhinoApp.RunScript(script, False)


def main():
    doc = sc.doc
    out = {"steps": {}}
    for i in [o.Id for o in doc.Objects]:
        doc.Objects.Delete(i, True)
    doc.ModelUnitSystem = Rhino.UnitSystem.Meters

    # the earth anchor point: Delft, 2 m above sea level, the model's north turned
    e = Rhino.DocObjects.EarthAnchorPoint()
    e.EarthBasepointLatitude = 52.011577
    e.EarthBasepointLongitude = 4.357068
    e.EarthBasepointElevation = 2.5
    e.EarthBasepointElevationZero = Rhino.DocObjects.BasepointZero.MeanSeaLevel
    e.ModelBasePoint = rg.Point3d(100, 200, 0)
    e.ModelNorth = rg.Vector3d(0.6, 0.8, 0)
    e.ModelEast = rg.Vector3d(0.8, -0.6, 0)
    e.Name = "Delft site"
    e.Description = "Anchor of the settings test"
    doc.EarthAnchorPoint = e

    # texts in their own fonts, a style in Courier New and one in Times New Roman italic
    courier = doc.DimStyles.Add("Courier style")
    s = doc.DimStyles[courier].Duplicate()
    s.Font = Rhino.DocObjects.Font("Courier New")
    s.TextHeight = 1.5
    doc.DimStyles.Modify(s, courier, True)
    times = doc.DimStyles.Add("Times italic")
    s = doc.DimStyles[times].Duplicate()
    s.Font = Rhino.DocObjects.Font("Times New Roman", Rhino.DocObjects.Font.FontWeight.Normal, Rhino.DocObjects.Font.FontStyle.Italic, False, False)
    doc.DimStyles.Modify(s, times, True)
    names = {}
    names[doc.Objects.AddText("Arial bold", rg.Plane(rg.Point3d(0, 0, 0), rg.Vector3d.ZAxis), 2.0, "Arial", True, False)] = "Arial bold"
    names[doc.Objects.AddText("Times italic", rg.Plane(rg.Point3d(0, 5, 0), rg.Vector3d.ZAxis), 2.0, "Times New Roman", False, True)] = "Times italic"
    t = rg.TextEntity.Create("Courier style", rg.Plane(rg.Point3d(0, 10, 0), rg.Vector3d.ZAxis), doc.DimStyles[courier], False, 0, 0)
    names[doc.Objects.AddText(t)] = "Courier style"
    t = rg.TextEntity.Create("Default style", rg.Plane(rg.Point3d(0, 15, 0), rg.Vector3d.ZAxis), doc.DimStyles.Current, False, 0, 0)
    names[doc.Objects.AddText(t)] = "Default style"
    d = rg.LinearDimension.Create(Rhino.Geometry.AnnotationType.Aligned, doc.DimStyles[times], rg.Plane.WorldXY, rg.Vector3d.XAxis,
                                  rg.Point3d(0, -5, 0), rg.Point3d(10, -5, 0), rg.Point3d(5, -8, 0), 0)
    names[doc.Objects.AddLinearDimension(d)] = "Times dimension"
    for i, n in names.items():
        a = doc.Objects.FindId(i).Attributes.Duplicate()
        a.Name = n
        doc.Objects.ModifyAttributes(i, a, True)

    # two named construction planes, and the Top view's moved
    doc.NamedConstructionPlanes.Add("Tilted", rg.Plane(rg.Point3d(1, 2, 3), rg.Vector3d(1, 0, 1)))
    doc.NamedConstructionPlanes.Add("Raised", rg.Plane(rg.Point3d(0, 0, 4), rg.Vector3d.ZAxis))
    for v in doc.Views:
        if v.ActiveViewport.Name == "Top":
            c = v.ActiveViewport.GetConstructionPlane()
            c.Plane = rg.Plane(rg.Point3d(5, 6, 0), rg.Vector3d.ZAxis)
            c.GridSpacing = 2.5
            c.SnapSpacing = 0.5
            c.GridLineCount = 40
            c.ThickLineFrequency = 4
            v.ActiveViewport.SetConstructionPlane(c)
            doc.Views.ActiveView = v

    # layers and two named layer states
    walls = doc.Layers.Add("Walls", C(200, 0, 0))
    roofs = doc.Layers.Add("Roofs", C(0, 0, 200))
    vp = doc.Views.ActiveView.ActiveViewport.Id
    out["steps"]["state_all"] = doc.NamedLayerStates.Save("All on", vp)
    doc.Layers[roofs].IsVisible = False
    out["steps"]["state_walls"] = doc.NamedLayerStates.Save("Walls only", vp)
    doc.Layers[roofs].IsVisible = True

    # an object with history: a circle extruded with History on
    run("_History _Record=_Yes _Update=_Yes _Lock=_No _BrokenRecordWarning=_No _Enter")
    circle = doc.Objects.AddCircle(rg.Circle(rg.Point3d(20, 0, 0), 3))
    doc.Objects.UnselectAll()
    doc.Objects.Select(circle)
    out["steps"]["extrude"] = run("_ExtrudeCrv 5")
    out["circle"] = str(circle)

    # a snapshot
    out["steps"]["snapshot"] = run("_-Snapshots _Save \"Snap one\" _Enter")

    # document user text: a key and a section entry
    doc.Strings.SetString("Project", "Settings test")
    doc.Strings.SetString("Site", "Parcel", "A-12")

    # custom render mesh settings
    mp = Rhino.Geometry.MeshingParameters(0.5)
    mp.MaximumEdgeLength = 3.0
    mp.MinimumEdgeLength = 0.01
    mp.GridAngle = 0.3
    mp.GridAspectRatio = 6.0
    mp.Tolerance = 0.05
    mp.RefineGrid = False
    mp.SimplePlanes = True
    doc.SetCustomMeshingParameters(mp)
    doc.MeshingParameterStyle = Rhino.Geometry.MeshingParameterStyle.Custom

    path = os.path.join(OUT, "settings.3dm")
    doc.WriteFile(path, Rhino.FileIO.FileWriteOptions())

    # what Rhino reads back
    f = Rhino.FileIO.File3dm.Read(path)
    a = f.Settings.EarthAnchorPoint if hasattr(f.Settings, "EarthAnchorPoint") else f.EarthAnchorPoint
    out["earth_anchor"] = {"latitude": a.EarthBasepointLatitude, "longitude": a.EarthBasepointLongitude,
                           "elevation": a.EarthBasepointElevation, "elevation_zero": int(a.EarthBasepointElevationZero),
                           "coordinate_system": int(a.EarthBasepointElevationCoordinateSystem),
                           "model_point": xyz(a.ModelBasePoint), "model_north": xyz(a.ModelNorth), "model_east": xyz(a.ModelEast),
                           "name": a.Name, "description": a.Description,
                           "kml_heading": a.KMLOrientationHeadingAngleDegrees, "kml_tilt": a.KMLOrientationTiltAngleDegrees,
                           "kml_roll": a.KMLOrientationRollAngleDegrees}
    st = f.Settings
    out["model_basepoint"] = xyz(st.ModelBasepoint)
    out["model_url"] = st.ModelUrl
    out["linetype_display_scale"] = st.LinetypeDisplayScale
    out["active_view"] = str(st.ActiveViewId)
    out["current"] = {"layer": str(st.CurrentLayerId), "material": str(st.CurrentRenderMaterialId),
                      "linetype": str(st.CurrentLinePatternId), "text_style": str(st.CurrentTextStyleId),
                      "dimension_style": str(st.CurrentDimensionStyleId), "hatch_pattern": str(st.CurrentHatchPatternId),
                      "color": color(st.CurrentColor), "plot_color": color(st.CurrentPlotColor),
                      "wire_density": st.CurrentWireDensity}
    out["save_texture_bitmaps"] = st.SaveTextureBitmapsInFile
    out["idef_link_update"] = int(st.InstanceDefinitionLinkUpdate)
    out["render_mesh"] = meshing(st.MeshingParameters)
    out["analysis_mesh"] = meshing(st.AnalysisMeshingParameters)
    out["custom_render_mesh"] = meshing(st.CustomRenderMeshingParameters)
    g = st.GridDefaults
    out["grid"] = {"grid_spacing": g.GridSpacing, "snap_spacing": g.SnapSpacing, "grid_line_count": g.GridLineCount,
                   "thick_frequency": g.GridThickFrequency, "show_grid": g.ShowGrid, "show_axes": g.ShowGridAxes,
                   "show_world_axes": g.ShowWorldAxes}
    r = st.RenderSettings
    out["render"] = {"ambient": color(r.AmbientLight), "background_top": color(r.BackgroundColorTop),
                     "background_bottom": color(r.BackgroundColorBottom), "background_style": int(r.BackgroundStyle),
                     "use_hidden_lights": r.UseHiddenLights, "depth_cue": r.DepthCue, "flat_shade": r.FlatShade,
                     "render_backfaces": r.RenderBackfaces, "render_points": r.RenderPoints, "render_curves": r.RenderCurves,
                     "render_isoparams": r.RenderIsoparams, "render_mesh_edges": r.RenderMeshEdges,
                     "render_annotations": r.RenderAnnotations, "antialias": int(r.AntialiasLevel),
                     "use_viewport_size": r.UseViewportSize, "scale_background": r.ScaleBackgroundToFit,
                     "transparent_background": r.TransparentBackground, "image_unit": int(r.ImageUnitSystem),
                     "image_dpi": r.ImageDpi, "image_size": [r.ImageSize.Width, r.ImageSize.Height],
                     "shadowmap_level": r.ShadowmapLevel, "named_view": r.NamedView, "snapshot": r.Snapshot,
                     "specific_viewport": r.SpecificViewport, "render_source": int(r.RenderSource)}
    out["named_cplanes"] = [cplane(c) for c in f.AllNamedConstructionPlanes]
    out["views"] = []
    for v in doc.Views:
        vp = v.ActiveViewport
        out["views"].append({"id": str(vp.Id), "name": vp.Name, "cplane": cplane(vp.GetConstructionPlane())})
    out["layer_states"] = list(doc.NamedLayerStates.Names)
    out["strings"] = [[f.Strings.GetKey(i), f.Strings.GetValue(i)] for i in range(f.Strings.Count)]
    out["plugin_data"] = [str(p.PlugInId) for p in f.PlugInData]
    out["history"] = []
    for o in doc.Objects:
        try:
            if o.HasHistoryRecord():
                h = o.HistoryParents
                h = h() if callable(h) else h
                out["history"].append({"id": str(o.Id), "parents": [str(p) for p in h]})
        except Exception as ex:
            out["steps"]["history_error"] = str(ex)
    out["texts"] = []
    for o in f.Objects:
        geo = o.Geometry
        if isinstance(geo, rg.AnnotationBase):
            out["texts"].append({"name": o.Attributes.Name, "id": str(o.Attributes.ObjectId), "font": font(geo.Font),
                                 "first_char_font": font(geo.FirstCharFont), "all_bold": geo.IsAllBold(), "all_italic": geo.IsAllItalic(),
                                 "style": str(geo.DimensionStyleId), "rich": geo.RichText, "plain": geo.PlainText})
    out["dim_styles"] = [{"name": s.Name, "id": str(s.Id), "index": s.Index, "font": font(s.Font)} for s in f.AllDimStyles]
    with open(os.path.join(OUT, "settings.json"), "w") as fh:
        json.dump(out, fh, indent=1)
    doc.Modified = False


try:
    main()
except Exception:
    import traceback
    with open(os.path.join(OUT, "settings_error.txt"), "w") as fh:
        fh.write(traceback.format_exc())
