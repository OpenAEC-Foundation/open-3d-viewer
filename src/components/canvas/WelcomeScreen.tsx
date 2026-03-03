import { useAppStore } from '../../state/appStore';
import { isSupported } from '../../engine/loaders/LoaderRegistry';

interface SampleFile {
  name: string;
  path: string;
  format: string;
  description: string;
  size: string;
  color: string;
}

const SAMPLES: SampleFile[] = [
  { name: 'Kavel 3.09', path: '/test-kavel.skp', format: 'SKP', description: 'SketchUp bouwproject', size: '2.1 MB', color: '#e44' },
  { name: 'Cube', path: '/samples/test-cube.stl', format: 'STL', description: 'Stereolithography kubus', size: '2.5 KB', color: '#4a9' },
  { name: 'Cube', path: '/samples/test-cube.obj', format: 'OBJ', description: 'Wavefront 3D object', size: '1.6 KB', color: '#49e' },
  { name: 'Sphere', path: '/samples/test-sphere.3mf', format: '3MF', description: '3D Manufacturing Format', size: '34 KB', color: '#e8a' },
  { name: 'Cube', path: '/samples/test-cube.wrl', format: 'WRL', description: 'VRML wereld', size: '12 KB', color: '#a6e' },
  { name: 'Point Cloud', path: '/samples/test-pointcloud.xyz', format: 'XYZ', description: 'Puntenwolk', size: '202 B', color: '#6be' },
  { name: 'Cube', path: '/samples/test-cube.vtk', format: 'VTK', description: 'Visualization Toolkit mesh', size: '248 B', color: '#ea6' },
  { name: 'CNC Toolpath', path: '/samples/test.gcode', format: 'GCODE', description: '3D-print / CNC instructies', size: '794 B', color: '#6a6' },
  { name: 'Mesh', path: '/samples/test.amf', format: 'AMF', description: 'Additive Manufacturing File', size: '924 B', color: '#c86' },
  { name: 'Plattegrond', path: '/samples/test-floorplan.dxf', format: 'DXF', description: 'AutoCAD tekening', size: '1.1 MB', color: '#88e' },
];

export function WelcomeScreen() {
  const models = useAppStore((s) => s.models);

  // Don't show if models are loaded
  if (models.length > 0) return null;

  const loadSample = async (sample: SampleFile) => {
    const ext = sample.path.substring(sample.path.lastIndexOf('.')).toLowerCase();
    if (!isSupported(ext)) return;

    const id = crypto.randomUUID();
    const format = ext.replace('.', '').toUpperCase();
    const fileName = sample.path.split('/').pop() || 'unknown';

    useAppStore.getState().addModel({
      id,
      fileName,
      filePath: fileName,
      format,
      vertices: 0,
      triangles: 0,
      bounds: { minX: 0, minY: 0, minZ: 0, maxX: 0, maxY: 0, maxZ: 0 },
      visible: true,
      loadingProgress: 0,
      loadingPhase: 'Laden...',
    });
    useAppStore.getState().addLoadingFile(id, fileName);

    try {
      const res = await fetch(sample.path);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const blob = await res.blob();
      const file = new File([blob], fileName, { type: 'application/octet-stream' });
      window.dispatchEvent(new CustomEvent('open3d:import-file', { detail: { id, file, format: ext } }));
    } catch (err) {
      console.error(`Failed to load sample: ${err}`);
      useAppStore.getState().setLoadingFileError(id, String(err));
    }
  };

  return (
    <div
      style={{
        position: 'absolute',
        inset: 0,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 5,
        pointerEvents: 'none',
      }}
    >
      <div style={{ pointerEvents: 'auto', maxWidth: 720, width: '100%', padding: '0 24px' }}>
        {/* Header */}
        <div style={{ textAlign: 'center', marginBottom: 28 }}>
          <h1 style={{ fontSize: 22, fontWeight: 700, color: 'rgba(200, 210, 230, 0.95)', margin: 0, letterSpacing: 0.5 }}>
            Open 3D Viewer Studio
          </h1>
          <p style={{ fontSize: 12, color: 'rgba(140, 155, 180, 0.8)', marginTop: 6 }}>
            Sleep een bestand hierheen of kies een voorbeeld
          </p>
        </div>

        {/* Sample grid */}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))',
            gap: 10,
          }}
        >
          {SAMPLES.map((sample, i) => (
            <button
              key={i}
              onClick={() => loadSample(sample)}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 12,
                padding: '10px 14px',
                background: 'rgba(25, 35, 55, 0.7)',
                border: '1px solid rgba(70, 100, 150, 0.25)',
                borderRadius: 6,
                cursor: 'pointer',
                textAlign: 'left',
                transition: 'background 0.15s, border-color 0.15s, transform 0.1s',
              }}
              onMouseEnter={(e) => {
                const el = e.currentTarget;
                el.style.background = 'rgba(35, 55, 90, 0.85)';
                el.style.borderColor = 'rgba(90, 130, 200, 0.5)';
                el.style.transform = 'translateY(-1px)';
              }}
              onMouseLeave={(e) => {
                const el = e.currentTarget;
                el.style.background = 'rgba(25, 35, 55, 0.7)';
                el.style.borderColor = 'rgba(70, 100, 150, 0.25)';
                el.style.transform = 'none';
              }}
            >
              {/* Format badge */}
              <div
                style={{
                  flexShrink: 0,
                  width: 40,
                  height: 40,
                  borderRadius: 6,
                  background: sample.color + '22',
                  border: `1px solid ${sample.color}44`,
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  fontSize: 10,
                  fontWeight: 800,
                  color: sample.color,
                  letterSpacing: 0.3,
                }}
              >
                {sample.format}
              </div>

              {/* Info */}
              <div style={{ minWidth: 0, flex: 1 }}>
                <div style={{ fontSize: 12, fontWeight: 600, color: 'rgba(200, 210, 230, 0.9)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                  {sample.name}
                </div>
                <div style={{ fontSize: 10, color: 'rgba(130, 145, 170, 0.8)', marginTop: 2 }}>
                  {sample.description}
                </div>
                <div style={{ fontSize: 9, color: 'rgba(110, 125, 150, 0.6)', marginTop: 1 }}>
                  {sample.size}
                </div>
              </div>
            </button>
          ))}
        </div>

        {/* Supported formats */}
        <div style={{ textAlign: 'center', marginTop: 20 }}>
          <p style={{ fontSize: 10, color: 'rgba(110, 125, 150, 0.6)' }}>
            Ondersteunde formaten: GLB, OBJ, STL, FBX, SKP, STEP, DXF, IFC, 3MF, DAE, PLY, PCD, WRL, XYZ, VTK, GCode, KMZ, VOX, AMF, BLEND, IGES en meer
          </p>
        </div>
      </div>
    </div>
  );
}
