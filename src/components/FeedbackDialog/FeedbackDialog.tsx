import { useState, useRef, useEffect } from 'react';
import { Paperclip } from 'lucide-react';
import { useTranslation } from '../../i18n/useTranslation';
import './FeedbackDialog.css';

type Category = 'general' | 'bug' | 'feature';
type Sentiment = 'Frustrated' | 'Neutral' | 'Happy';

interface ImageAttachment {
  file: File;
  preview: string;
}

const FEEDBACK_API = 'https://open-feedback-studio.pages.dev/api/feedback';
const MAX_IMAGES = 3;
const MAX_TOTAL_SIZE = 1024 * 1024; // 1MB

interface FeedbackDialogProps {
  onClose: () => void;
}

export function FeedbackDialog({ onClose }: FeedbackDialogProps) {
  const { t } = useTranslation('feedback');

  const [category, setCategory] = useState<Category>('general');
  const [message, setMessage] = useState('');
  const [images, setImages] = useState<ImageAttachment[]>([]);
  const [sentiment, setSentiment] = useState<Sentiment | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState(false);

  const fileInputRef = useRef<HTMLInputElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);

  // Dragging
  const [offset, setOffset] = useState<{ x: number; y: number } | null>(null);
  const dragging = useRef(false);
  const dragStart = useRef({ x: 0, y: 0 });

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!dragging.current) return;
      setOffset({
        x: e.clientX - dragStart.current.x,
        y: e.clientY - dragStart.current.y,
      });
    };
    const handleMouseUp = () => { dragging.current = false; };
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
    };
  }, []);

  // Escape to close
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  const handleHeaderMouseDown = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('.feedback-close')) return;
    dragging.current = true;
    dragStart.current = {
      x: e.clientX - (offset?.x ?? 0),
      y: e.clientY - (offset?.y ?? 0),
    };
  };

  const totalImageSize = images.reduce((sum, img) => sum + img.file.size, 0);

  const handleImageAdd = () => {
    fileInputRef.current?.click();
  };

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files) return;

    const newImages = [...images];
    for (const file of Array.from(files)) {
      if (newImages.length >= MAX_IMAGES) break;
      if (!file.type.startsWith('image/')) continue;
      const newTotal = newImages.reduce((s, img) => s + img.file.size, 0) + file.size;
      if (newTotal > MAX_TOTAL_SIZE) continue;
      newImages.push({ file, preview: URL.createObjectURL(file) });
    }
    setImages(newImages);
    e.target.value = '';
  };

  const removeImage = (index: number) => {
    setImages((prev) => {
      const copy = [...prev];
      URL.revokeObjectURL(copy[index].preview);
      copy.splice(index, 1);
      return copy;
    });
  };

  const canSubmit = message.trim().length >= 10 && !submitting;

  const handleSubmit = async () => {
    if (!canSubmit) return;
    setSubmitting(true);
    setError('');

    try {
      let body: FormData | string;
      const headers: Record<string, string> = {};

      if (images.length > 0) {
        const formData = new FormData();
        formData.append('app', 'open-3d-viewer');
        formData.append('category', category);
        formData.append('message', message.trim());
        if (sentiment) formData.append('sentiment', sentiment);
        formData.append('appVersion', `${__APP_VERSION__}+${__APP_BUILD__}`);
        for (const img of images) {
          formData.append('images', img.file);
        }
        body = formData;
      } else {
        headers['Content-Type'] = 'application/json';
        body = JSON.stringify({
          app: 'open-3d-viewer',
          category,
          message: message.trim(),
          sentiment: sentiment ?? undefined,
          appVersion: `${__APP_VERSION__}+${__APP_BUILD__}`,
        });
      }

      const res = await fetch(FEEDBACK_API, { method: 'POST', headers, body });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      setSuccess(true);
    } catch {
      setError(t('errorGeneric'));
    } finally {
      setSubmitting(false);
    }
  };

  const handleSendAnother = () => {
    setCategory('general');
    setMessage('');
    setImages([]);
    setSentiment(null);
    setError('');
    setSuccess(false);
  };

  return (
    <div className="feedback-dialog-overlay">
      <div
        ref={dialogRef}
        className="feedback-dialog"
        style={offset ? { transform: `translate(${offset.x}px, ${offset.y}px)` } : undefined}
      >
        {/* Header */}
        <div className="feedback-header" onMouseDown={handleHeaderMouseDown}>
          <h2 className="feedback-title">{t('title')}</h2>
          <button className="feedback-close" onClick={onClose}>&times;</button>
        </div>

        {success ? (
          /* Success view */
          <div className="feedback-success">
            <span className="feedback-success-icon">&#10004;</span>
            <h3 className="feedback-success-title">{t('successTitle')}</h3>
            <p className="feedback-success-message">{t('successMessage')}</p>
            <button className="feedback-send-another" onClick={handleSendAnother}>
              {t('sendAnother')}
            </button>
          </div>
        ) : (
          <>
            {/* Body */}
            <div className="feedback-body">
              {/* Category */}
              <div className="feedback-categories">
                {(['general', 'bug', 'feature'] as Category[]).map((cat) => (
                  <button
                    key={cat}
                    className={`feedback-category-btn${category === cat ? ' active' : ''}`}
                    onClick={() => setCategory(cat)}
                  >
                    {t(`category${cat.charAt(0).toUpperCase() + cat.slice(1)}`)}
                  </button>
                ))}
              </div>

              {/* Message */}
              <textarea
                className="feedback-textarea"
                placeholder={t('messagePlaceholder')}
                value={message}
                onChange={(e) => setMessage(e.target.value.slice(0, 5000))}
                maxLength={5000}
              />
              <span className="feedback-char-count">{message.length}/5000</span>

              {/* Images */}
              <div className="feedback-images-section">
                <div className="feedback-images-header">
                  <button
                    className="feedback-attach-btn"
                    onClick={handleImageAdd}
                    disabled={images.length >= MAX_IMAGES || totalImageSize >= MAX_TOTAL_SIZE}
                  >
                    <Paperclip size={12} />
                    {t('attachImages')}
                  </button>
                  <span className="feedback-image-limit">{t('imageLimit')}</span>
                </div>
                {images.length > 0 && (
                  <div className="feedback-image-previews">
                    {images.map((img, i) => (
                      <div key={i} className="feedback-image-thumb">
                        <img src={img.preview} alt="" />
                        <button className="feedback-image-remove" onClick={() => removeImage(i)}>&times;</button>
                      </div>
                    ))}
                  </div>
                )}
                <input
                  ref={fileInputRef}
                  type="file"
                  accept="image/*"
                  multiple
                  style={{ display: 'none' }}
                  onChange={handleFileChange}
                />
              </div>

              {/* Sentiment */}
              <div className="feedback-sentiment-section">
                <span className="feedback-sentiment-label">{t('sentiment')}</span>
                <div className="feedback-sentiments">
                  {([
                    { value: 'Frustrated' as Sentiment, emoji: '\uD83D\uDE1E', key: 'sentimentFrustrated' },
                    { value: 'Neutral' as Sentiment, emoji: '\uD83D\uDE10', key: 'sentimentNeutral' },
                    { value: 'Happy' as Sentiment, emoji: '\uD83D\uDE0A', key: 'sentimentHappy' },
                  ]).map((s) => (
                    <button
                      key={s.value}
                      className={`feedback-sentiment-btn${sentiment === s.value ? ' active' : ''}`}
                      onClick={() => setSentiment(sentiment === s.value ? null : s.value)}
                    >
                      <span className="feedback-sentiment-emoji">{s.emoji}</span>
                      {t(s.key)}
                    </button>
                  ))}
                </div>
              </div>
            </div>

            {/* Footer */}
            <div className="feedback-footer">
              {error && <span className="feedback-error">{error}</span>}
              <button
                className="feedback-submit-btn"
                onClick={handleSubmit}
                disabled={!canSubmit}
              >
                {submitting ? t('submitting') : t('submit')}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
