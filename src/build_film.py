# Run by Blender without a window (see build_film.rs):
#   blender --background --factory-startup --python build_film.py -- STAGE.usda PLAN.json OUT.mp4
# Loads the stage, keys what shows from which frame, puts a sun and a camera that turns round the
# model, and writes the film.
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

# the film
scene.frame_start, scene.frame_end = 1, plan["end"]
render = scene.render
render.fps = plan["fps"]
render.resolution_x, render.resolution_y, render.resolution_percentage = 1920, 1080, 100
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
