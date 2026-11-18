# Run by Blender without a window (see build_film.rs):
#   blender --background --factory-startup --python build_film.py -- STAGE.usda PLAN.json OUT.mp4
# Loads the stage, keys what shows from which frame, puts a sun and a camera that turns round the
# model, and writes the film. With "still" in the plan: one image of the whole model, path traced,
# with ground under it and a sky, from the camera the plan gives or one placed here.
import json
import math
import sys

import bpy
from mathutils import Vector

stage, plan_file, out = sys.argv[sys.argv.index("--") + 1:][:3]
with open(plan_file, encoding="utf-8") as f:
    plan = json.load(f)

# an empty scene
bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
bpy.ops.wm.usd_import(filepath=stage)
meshes = {o.name: o for o in scene.objects if o.type == "MESH"}
if not meshes:
    sys.exit("the stage holds no meshes")

# every mesh in the colour of its element (sRGB in the plan, linear in the shader)
def linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


for o in meshes.values():
    material = bpy.data.materials.new(o.name)
    material.use_nodes = True
    shader = material.node_tree.nodes.get("Principled BSDF")
    c = plan.get("colours", {}).get(o.name, [0.8, 0.8, 0.8])
    if shader:
        shader.inputs["Base Color"].default_value = (linear(c[0]), linear(c[1]), linear(c[2]), 1.0)
        shader.inputs["Roughness"].default_value = 0.8
    o.data.materials.clear()
    o.data.materials.append(material)


def key(o, hidden, frame):
    o.hide_render = hidden
    o.hide_viewport = hidden
    o.keyframe_insert("hide_render", frame=frame)
    o.keyframe_insert("hide_viewport", frame=frame)


# what shows when: hidden from the first frame, shown from its step on, gone from the step that
# removes it (keys hold their value until the next one)
first = {}
for step in plan["steps"]:
    for name in step["show"]:
        first.setdefault(name, step["frame"])
for name, frame in first.items():
    o = meshes.get(name)
    if o is None:
        continue
    if frame > 1:
        key(o, True, 1)
    key(o, False, frame)
for step in plan["steps"]:
    for name in step["hide"]:
        o = meshes.get(name)
        if o is not None and step["frame"] > first.get(name, 1):
            key(o, True, step["frame"])
for o in meshes.values():
    if o.animation_data and o.animation_data.action:
        for curve in getattr(o.animation_data.action, "fcurves", []):
            for point in curve.keyframe_points:
                point.interpolation = "CONSTANT"

# the box round the whole model
corners = [o.matrix_world @ Vector(c) for o in meshes.values() for c in o.bound_box]
low = Vector((min(c.x for c in corners), min(c.y for c in corners), min(c.z for c in corners)))
high = Vector((max(c.x for c in corners), max(c.y for c in corners), max(c.z for c in corners)))
centre, size = (low + high) / 2, max((high - low).length, 1.0)

# a camera on an arm that turns a third of the way round while the model is built
pivot = bpy.data.objects.new("pivot", None)
pivot.location = centre
scene.collection.objects.link(pivot)
camera = bpy.data.objects.new("camera", bpy.data.cameras.new("camera"))
scene.collection.objects.link(camera)
camera.parent = pivot
camera.data.lens = 35
camera.data.clip_end = size * 20
distance = size * 1.25
camera.location = Vector((distance * 0.75, -distance * 0.75, distance * 0.45))
aim = camera.constraints.new("TRACK_TO")
aim.target = pivot
aim.track_axis = "TRACK_NEGATIVE_Z"
aim.up_axis = "UP_Y"
scene.camera = camera
still = bool(plan.get("still"))
given = plan.get("camera")
if given:
    # the camera of the caller: where its eye is, what it looks at, how wide it sees
    pivot.location = Vector(given["target"])
    camera.location = Vector(given["eye"]) - Vector(given["target"])
    camera.data.sensor_fit = "VERTICAL"
    camera.data.angle_y = math.radians(given["fov"])
    camera.data.clip_end = max(size * 20, camera.location.length * 4)
elif still:
    pivot.rotation_euler = (0, 0, math.radians(20))
else:
    pivot.rotation_euler = (0, 0, math.radians(-25))
    pivot.keyframe_insert("rotation_euler", frame=1)
    pivot.rotation_euler = (0, 0, math.radians(95))
    pivot.keyframe_insert("rotation_euler", frame=plan["end"])

# light: a sun, and a light sky that fills the shadows
sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
sun.data.energy = 3.0
sun.rotation_euler = (math.radians(50), 0, math.radians(30))
scene.collection.objects.link(sun)
world = bpy.data.worlds.new("sky")
world.use_nodes = True
background = world.node_tree.nodes.get("Background")
if background:
    background.inputs["Color"].default_value = (1.0, 1.0, 1.0, 1.0)
    background.inputs["Strength"].default_value = 1.0
scene.world = world

scene.frame_start, scene.frame_end = 1, plan["end"]
render = scene.render
render.fps = plan["fps"]
render.resolution_x, render.resolution_y, render.resolution_percentage = 1920, 1080, 100

if still:
    # ground under the model, to stand on and to catch its shadow
    ground = bpy.data.meshes.new("ground")
    r = size * 40
    ground.from_pydata([(-r, -r, 0), (r, -r, 0), (r, r, 0), (-r, r, 0)], [], [(0, 1, 2, 3)])
    floor = bpy.data.objects.new("ground", ground)
    floor.location = (centre.x, centre.y, low.z - 0.002)
    scene.collection.objects.link(floor)
    earth = bpy.data.materials.new("ground")
    earth.use_nodes = True
    shader = earth.node_tree.nodes.get("Principled BSDF")
    if shader:
        shader.inputs["Base Color"].default_value = (0.42, 0.42, 0.4, 1.0)
        shader.inputs["Roughness"].default_value = 0.95
    ground.materials.append(earth)
    # a sky that lights the scene, with the sun in it
    nodes, links = world.node_tree.nodes, world.node_tree.links
    sky = nodes.new("ShaderNodeTexSky")
    try:
        sky.sun_elevation = math.radians(40)
        sky.sun_rotation = math.radians(150)
    except AttributeError:
        pass
    if background:
        links.new(sky.outputs["Color"], background.inputs["Color"])
        background.inputs["Strength"].default_value = 0.06
    sun.data.energy = 2.2
    sun.data.angle = math.radians(2)
    # the path tracer, on the graphics card where Blender finds one
    render.engine = "CYCLES"
    scene.cycles.samples = 96
    scene.cycles.use_denoising = True
    try:
        prefs = bpy.context.preferences.addons["cycles"].preferences
        for kind in ("OPTIX", "CUDA", "HIP", "ONEAPI", "METAL"):
            try:
                prefs.compute_device_type = kind
                prefs.get_devices()
                if any(d.type == kind for d in prefs.devices):
                    for d in prefs.devices:
                        d.use = True
                    scene.cycles.device = "GPU"
                    break
            except Exception:
                continue
    except Exception:
        pass
    scene.view_settings.view_transform = "AgX" if "AgX" in [t.identifier for t in type(scene.view_settings).bl_rna.properties["view_transform"].enum_items] else "Filmic"
    try:
        render.image_settings.media_type = "IMAGE"
    except (AttributeError, TypeError):
        pass
    render.image_settings.file_format = "PNG"
    render.filepath = out
    bpy.ops.render.render(write_still=True)
    sys.exit(0)

# the film
engines = [e.identifier for e in type(render).bl_rna.properties["engine"].enum_items]
render.engine = "BLENDER_EEVEE" if "BLENDER_EEVEE" in engines else "BLENDER_EEVEE_NEXT"
render.filepath = out
# colours as they are given, not toned down
scene.view_settings.view_transform = "Standard"
try:
    render.image_settings.media_type = "VIDEO"
except (AttributeError, TypeError):
    pass
render.image_settings.file_format = "FFMPEG"
render.ffmpeg.format = "MPEG4"
render.ffmpeg.codec = "H264"
render.ffmpeg.constant_rate_factor = "HIGH"
bpy.ops.render.render(animation=True)
