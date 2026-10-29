%global debug_package %{nil}
%global __strip /bin/true
%define _build_id_links none

Name:           open-3d-viewer
Version:        %{number}
Release:        1
Summary:        3D model viewer for AEC files
License:        LGPL-3.0-or-later
URL:            https://github.com/OpenAEC-Foundation/open-3d-viewer
BuildArch:      x86_64

%description
Open 3D Viewer Studio opens SketchUp, Tekla, glTF, OBJ, STL and PLY models,
measures and sections them, and saves them as IFC.

%install
install -Dm755 %{_sourcedir}/open-3d-viewer %{buildroot}/usr/bin/open-3d-viewer
install -Dm755 %{_sourcedir}/open-3d-viewer-mcp %{buildroot}/usr/bin/open-3d-viewer-mcp
install -Dm644 %{_sourcedir}/org.openaec.Open3DViewer.desktop %{buildroot}/usr/share/applications/org.openaec.Open3DViewer.desktop
install -Dm644 %{_sourcedir}/icon.png %{buildroot}/usr/share/icons/hicolor/512x512/apps/org.openaec.Open3DViewer.png
install -Dm644 %{_sourcedir}/LICENSE %{buildroot}/usr/share/licenses/open-3d-viewer/LICENSE

%files
/usr/bin/open-3d-viewer
/usr/bin/open-3d-viewer-mcp
/usr/share/applications/org.openaec.Open3DViewer.desktop
/usr/share/icons/hicolor/512x512/apps/org.openaec.Open3DViewer.png
/usr/share/licenses/open-3d-viewer/LICENSE
