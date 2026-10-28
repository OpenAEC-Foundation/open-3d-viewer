import { useAppStore } from '../../state/appStore';
import { isSupported } from '../../engine/loaders/LoaderRegistry';
import { useTranslation } from '../../i18n/useTranslation';
import './WelcomeScreen.css';

interface SampleFile {
  name: string;
  path: string;
  format: string;
  description: string;
  size: string;
}

const SAMPLES: SampleFile[] = [
  { name: 'Plot 3.09', path: '/test-kavel.skp', format: 'SKP', description: 'SketchUp building project', size: '2.1 MB' },
  { name: 'Cube', path: '/samples/test-cube.stl', format: 'STL', description: 'Stereolithography cube', size: '2.5 KB' },
  { name: 'Cube', path: '/samples/test-cube.obj', format: 'OBJ', description: 'Wavefront 3D object', size: '1.6 KB' },
  { name: 'Sphere', path: '/samples/test-sphere.3mf', format: '3MF', description: '3D Manufacturing Format', size: '34 KB' },
  { name: 'Cube', path: '/samples/test-cube.wrl', format: 'WRL', description: 'VRML world', size: '12 KB' },
  { name: 'Point Cloud', path: '/samples/test-pointcloud.xyz', format: 'XYZ', description: 'Point cloud', size: '202 B' },
  { name: 'Cube', path: '/samples/test-cube.vtk', format: 'VTK', description: 'Visualization Toolkit mesh', size: '248 B' },
  { name: 'CNC Toolpath', path: '/samples/test.gcode', format: 'GCODE', description: '3D print / CNC instructions', size: '794 B' },
  { name: 'Mesh', path: '/samples/test.amf', format: 'AMF', description: 'Additive Manufacturing File', size: '924 B' },
  { name: 'Floor Plan', path: '/samples/test-floorplan.dxf', format: 'DXF', description: 'AutoCAD drawing', size: '1.1 MB' },
];

export function WelcomeScreen() {
  const { t } = useTranslation('welcome');
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
      loadingPhase: t('loading'),
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
    <div className="welcome">
      <div className="welcome-inner">
        <div className="welcome-header">
          <h1>{t('title')}</h1>
          <p>{t('subtitle')}</p>
          <div className="welcome-accent" />
        </div>

        <div className="welcome-grid">
          {SAMPLES.map((sample, i) => (
            <button key={i} className="welcome-card" onClick={() => loadSample(sample)}>
              <span className="welcome-badge">{sample.format}</span>
              <div className="welcome-info">
                <div className="welcome-name">{sample.name}</div>
                <div className="welcome-desc">{sample.description}</div>
                <div className="welcome-size">{sample.size}</div>
              </div>
            </button>
          ))}
        </div>

        <p className="welcome-footer">{t('supportedFormats')}</p>
      </div>
    </div>
  );
}
