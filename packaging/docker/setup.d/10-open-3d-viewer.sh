# Open 3D Viewer Studio in the Docker image, sourced by openaec-start just before the viewer
# starts. The other files in packaging/docker are the OpenAEC Blueprint's template (templates/docker
# on the Blueprint branch feat/docker-image, commit 715730a3), copied unchanged; this one, the
# list of libraries the image adds for the viewer (extra-packages: Mesa's software Vulkan and
# OpenGL for the 3D view, zenity for the file dialogs without a desktop portal) and test-mcp.sh
# are the viewer's own part.
#
#   OPENAEC_LANG / OPENAEC_TAAL    nl | en (the viewer speaks Dutch and English; others get English)
#   OPENAEC_THEME / OPENAEC_THEMA  light | dark | blueprint | high-contrast-dark | high-contrast-light
#   OPENAEC_MCP_GEHEIM             secret (16+ characters) that switches the MCP server on
#   OPENAEC_MCP_POORT              its port, default 3001, for the internal network only

# Managed: no update notice (a new image brings the update), and dialogs, relative paths and the
# AI connection work in the user's folder.
export OPENAEC_BEHEERD=1
export OPENAEC_MAP="${OPENAEC_MAP:-/openaec}"

# Language and theme, under the template's names or the SuperCloud's; the viewer reads the latter.
if [ -n "${OPENAEC_LANG:-}" ] && [ -z "${OPENAEC_TAAL:-}" ]; then
  export OPENAEC_TAAL="$OPENAEC_LANG"
fi
if [ -n "${OPENAEC_THEME:-}" ] && [ -z "${OPENAEC_THEMA:-}" ]; then
  export OPENAEC_THEMA="$OPENAEC_THEME"
fi

# The 3D view draws with wgpu, here on Mesa's software renderers (lavapipe, llvmpipe) as there is
# no GPU. The template asks iced for its CPU renderer, which draws the window but not the 3D view;
# without ICED_BACKEND iced takes wgpu and falls back to the CPU renderer only when wgpu finds no
# adapter at all.
unset ICED_BACKEND
export LIBGL_ALWAYS_SOFTWARE=1

# The AI connection: the viewer serves MCP itself at http://<container>:<port>/mcp when a secret is
# given (Authorization: Bearer <secret>; requests with an Origin header are refused). The port is
# not published; the SuperCloud reaches it over the internal network.
if [ -n "${OPENAEC_MCP_GEHEIM:-}" ]; then
  export OPENAEC_MCP_POORT="${OPENAEC_MCP_POORT:-3001}"
fi
