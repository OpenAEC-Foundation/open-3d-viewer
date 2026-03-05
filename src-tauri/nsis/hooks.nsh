; ==========================================================================
; Open 3D Viewer Studio — NSIS Installer Hooks
; ==========================================================================

!include "nsDialogs.nsh"
!include "LogicLib.nsh"

; State variables (1 = checked)
Var FA_INIT
Var FA_COMMON
Var FA_CAD
Var FA_SCENE
Var FA_OTHER

; Dialog handles
Var FA_HW_COMMON
Var FA_HW_CAD
Var FA_HW_SCENE
Var FA_HW_OTHER
Var FA_HW_ALL

; ---------------------------------------------------------------------------
; File Association page: create
; ---------------------------------------------------------------------------
Function faPageCreate
  ${If} $PassiveMode = 1
  ${OrIf} $UpdateMode = 1
    StrCpy $FA_COMMON "1"
    StrCpy $FA_CAD "1"
    StrCpy $FA_SCENE "1"
    StrCpy $FA_OTHER "1"
    Abort
  ${EndIf}

  ; Initialize all checked on first visit
  ${If} $FA_INIT == ""
    StrCpy $FA_INIT "1"
    StrCpy $FA_COMMON "1"
    StrCpy $FA_CAD "1"
    StrCpy $FA_SCENE "1"
    StrCpy $FA_OTHER "1"
  ${EndIf}

  !insertmacro MUI_HEADER_TEXT "File Associations" "Choose which 3D file formats to open with ${PRODUCTNAME}."

  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0 0 100% 16u "Select the file types you want to associate with ${PRODUCTNAME}:"
  Pop $0

  ; --- Common 3D formats (most popular) ---
  ${NSD_CreateCheckbox} 12u 22u -12u 14u "Common 3D Files (.obj, .stl, .fbx, .gltf, .glb, .3mf, .ply, .dae)"
  Pop $FA_HW_COMMON
  ${If} $FA_COMMON == "1"
    ${NSD_Check} $FA_HW_COMMON
  ${EndIf}

  ; --- CAD / BIM ---
  ${NSD_CreateCheckbox} 12u 42u -12u 14u "CAD && BIM Files (.step, .stp, .iges, .igs, .dxf, .ifc)"
  Pop $FA_HW_CAD
  ${If} $FA_CAD == "1"
    ${NSD_Check} $FA_HW_CAD
  ${EndIf}

  ; --- Scene / Exchange ---
  ${NSD_CreateCheckbox} 12u 62u -12u 14u "Scene Files (.skp, .3ds, .x3d, .vrml, .wrl, .amf, .off)"
  Pop $FA_HW_SCENE
  ${If} $FA_SCENE == "1"
    ${NSD_Check} $FA_HW_SCENE
  ${EndIf}

  ; --- Other / Specialized ---
  ${NSD_CreateCheckbox} 12u 82u -12u 14u "Other Formats (.pcd, .xyz, .vox, .vtk, .vtp, .gcode, .blend, .lwo, .x, .kmz)"
  Pop $FA_HW_OTHER
  ${If} $FA_OTHER == "1"
    ${NSD_Check} $FA_HW_OTHER
  ${EndIf}

  ; --- Select All toggle ---
  ${NSD_CreateCheckbox} 0 108u -12u 14u "Select All / Deselect All"
  Pop $FA_HW_ALL
  ${NSD_Check} $FA_HW_ALL
  ${NSD_OnClick} $FA_HW_ALL faToggleAll

  nsDialogs::Show
FunctionEnd

; ---------------------------------------------------------------------------
; Toggle all checkboxes
; ---------------------------------------------------------------------------
Function faToggleAll
  ${NSD_GetState} $FA_HW_ALL $0
  ${NSD_SetState} $FA_HW_COMMON $0
  ${NSD_SetState} $FA_HW_CAD $0
  ${NSD_SetState} $FA_HW_SCENE $0
  ${NSD_SetState} $FA_HW_OTHER $0
FunctionEnd

; ---------------------------------------------------------------------------
; File Association page: save selections
; ---------------------------------------------------------------------------
Function faPageLeave
  ${NSD_GetState} $FA_HW_COMMON $FA_COMMON
  ${NSD_GetState} $FA_HW_CAD $FA_CAD
  ${NSD_GetState} $FA_HW_SCENE $FA_SCENE
  ${NSD_GetState} $FA_HW_OTHER $FA_OTHER
FunctionEnd

; ---------------------------------------------------------------------------
; Helper macros
; ---------------------------------------------------------------------------
!macro _RegExt ext progId desc
  WriteRegStr SHCTX "SOFTWARE\Classes\.${ext}" "" "${progId}"
  WriteRegStr SHCTX "SOFTWARE\Classes\${progId}" "" "${desc}"
  WriteRegStr SHCTX "SOFTWARE\Classes\${progId}\DefaultIcon" "" "$INSTDIR\${MAINBINARYNAME}.exe,0"
  WriteRegStr SHCTX "SOFTWARE\Classes\${progId}\shell" "" "open"
  WriteRegStr SHCTX "SOFTWARE\Classes\${progId}\shell\open" "" "Open with ${PRODUCTNAME}"
  WriteRegStr SHCTX "SOFTWARE\Classes\${progId}\shell\open\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" "%1"'
!macroend

!macro _UnregExt ext progId
  DeleteRegKey SHCTX "SOFTWARE\Classes\.${ext}"
  DeleteRegKey SHCTX "SOFTWARE\Classes\${progId}"
!macroend

; ---------------------------------------------------------------------------
; POSTINSTALL: register selected file associations
; ---------------------------------------------------------------------------
!macro NSIS_HOOK_POSTINSTALL

  ; --- Common 3D (OBJ, STL, FBX, glTF, 3MF, PLY, DAE) ---
  ${If} $FA_COMMON == ${BST_CHECKED}
    !insertmacro _RegExt "obj"  "Open3DViewer.obj"  "Wavefront OBJ File"
    !insertmacro _RegExt "stl"  "Open3DViewer.stl"  "STL 3D File"
    !insertmacro _RegExt "fbx"  "Open3DViewer.fbx"  "FBX Scene File"
    !insertmacro _RegExt "gltf" "Open3DViewer.gltf" "glTF File"
    !insertmacro _RegExt "glb"  "Open3DViewer.glb"  "glTF Binary File"
    !insertmacro _RegExt "3mf"  "Open3DViewer.3mf"  "3MF File"
    !insertmacro _RegExt "ply"  "Open3DViewer.ply"  "PLY Polygon File"
    !insertmacro _RegExt "dae"  "Open3DViewer.dae"  "Collada Scene File"
  ${EndIf}

  ; --- CAD & BIM (STEP, IGES, DXF, IFC) ---
  ${If} $FA_CAD == ${BST_CHECKED}
    !insertmacro _RegExt "step" "Open3DViewer.step" "STEP CAD File"
    !insertmacro _RegExt "stp"  "Open3DViewer.stp"  "STEP CAD File"
    !insertmacro _RegExt "iges" "Open3DViewer.iges" "IGES CAD File"
    !insertmacro _RegExt "igs"  "Open3DViewer.igs"  "IGES CAD File"
    !insertmacro _RegExt "dxf"  "Open3DViewer.dxf"  "DXF Drawing File"
    !insertmacro _RegExt "ifc"  "Open3DViewer.ifc"  "IFC BIM File"
  ${EndIf}

  ; --- Scene (SKP, 3DS, X3D, VRML, AMF, OFF) ---
  ${If} $FA_SCENE == ${BST_CHECKED}
    !insertmacro _RegExt "skp"  "Open3DViewer.skp"  "SketchUp File"
    !insertmacro _RegExt "3ds"  "Open3DViewer.3ds"  "3DS Scene File"
    !insertmacro _RegExt "x3d"  "Open3DViewer.x3d"  "X3D File"
    !insertmacro _RegExt "vrml" "Open3DViewer.vrml" "VRML File"
    !insertmacro _RegExt "wrl"  "Open3DViewer.wrl"  "VRML World File"
    !insertmacro _RegExt "amf"  "Open3DViewer.amf"  "AMF File"
    !insertmacro _RegExt "off"  "Open3DViewer.off"  "OFF Mesh File"
  ${EndIf}

  ; --- Other / Specialized ---
  ${If} $FA_OTHER == ${BST_CHECKED}
    !insertmacro _RegExt "pcd"   "Open3DViewer.pcd"   "PCD Point Cloud"
    !insertmacro _RegExt "xyz"   "Open3DViewer.xyz"   "XYZ Point Cloud"
    !insertmacro _RegExt "vox"   "Open3DViewer.vox"   "MagicaVoxel File"
    !insertmacro _RegExt "vtk"   "Open3DViewer.vtk"   "VTK File"
    !insertmacro _RegExt "vtp"   "Open3DViewer.vtp"   "VTP File"
    !insertmacro _RegExt "gcode" "Open3DViewer.gcode" "G-code File"
    !insertmacro _RegExt "blend" "Open3DViewer.blend" "Blender File"
    !insertmacro _RegExt "lwo"   "Open3DViewer.lwo"   "LightWave Object"
    !insertmacro _RegExt "x"     "Open3DViewer.x"     "DirectX 3D File"
    !insertmacro _RegExt "kmz"   "Open3DViewer.kmz"   "Google Earth KMZ"
  ${EndIf}

  ; Refresh Windows shell icon cache
  System::Call 'Shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)'

!macroend

; ---------------------------------------------------------------------------
; PREUNINSTALL: remove all file associations
; ---------------------------------------------------------------------------
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro _UnregExt "obj"   "Open3DViewer.obj"
  !insertmacro _UnregExt "stl"   "Open3DViewer.stl"
  !insertmacro _UnregExt "fbx"   "Open3DViewer.fbx"
  !insertmacro _UnregExt "gltf"  "Open3DViewer.gltf"
  !insertmacro _UnregExt "glb"   "Open3DViewer.glb"
  !insertmacro _UnregExt "3mf"   "Open3DViewer.3mf"
  !insertmacro _UnregExt "ply"   "Open3DViewer.ply"
  !insertmacro _UnregExt "dae"   "Open3DViewer.dae"
  !insertmacro _UnregExt "step"  "Open3DViewer.step"
  !insertmacro _UnregExt "stp"   "Open3DViewer.stp"
  !insertmacro _UnregExt "iges"  "Open3DViewer.iges"
  !insertmacro _UnregExt "igs"   "Open3DViewer.igs"
  !insertmacro _UnregExt "dxf"   "Open3DViewer.dxf"
  !insertmacro _UnregExt "ifc"   "Open3DViewer.ifc"
  !insertmacro _UnregExt "skp"   "Open3DViewer.skp"
  !insertmacro _UnregExt "3ds"   "Open3DViewer.3ds"
  !insertmacro _UnregExt "x3d"   "Open3DViewer.x3d"
  !insertmacro _UnregExt "vrml"  "Open3DViewer.vrml"
  !insertmacro _UnregExt "wrl"   "Open3DViewer.wrl"
  !insertmacro _UnregExt "amf"   "Open3DViewer.amf"
  !insertmacro _UnregExt "off"   "Open3DViewer.off"
  !insertmacro _UnregExt "pcd"   "Open3DViewer.pcd"
  !insertmacro _UnregExt "xyz"   "Open3DViewer.xyz"
  !insertmacro _UnregExt "vox"   "Open3DViewer.vox"
  !insertmacro _UnregExt "vtk"   "Open3DViewer.vtk"
  !insertmacro _UnregExt "vtp"   "Open3DViewer.vtp"
  !insertmacro _UnregExt "gcode" "Open3DViewer.gcode"
  !insertmacro _UnregExt "blend" "Open3DViewer.blend"
  !insertmacro _UnregExt "lwo"   "Open3DViewer.lwo"
  !insertmacro _UnregExt "x"     "Open3DViewer.x"
  !insertmacro _UnregExt "kmz"   "Open3DViewer.kmz"

  System::Call 'Shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)'
!macroend
