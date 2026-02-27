import { useEffect, useRef } from 'react';
import { motion } from 'framer-motion';
import { Check } from 'lucide-react';

interface Props {
  payData: { payment_id: number; qr_payload: string; amount: number };
  payStatus: { status: string } | null;
  onClose: () => void;
}

export default function QRCodeView({ payData, payStatus, onClose }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const isPaid = payStatus?.status === 'paid';

  useEffect(() => {
    if (!payData.qr_payload || !canvasRef.current) return;
    import('qrcode').then(QRCode => {
      QRCode.toCanvas(canvasRef.current!, payData.qr_payload, {
        width: 180,
        margin: 1,
        color: { dark: '#000000', light: '#ffffff' },
      });
    });
  }, [payData.qr_payload]);

  if (isPaid) {
    return (
      <motion.div
        initial={{ opacity: 0, scale: 0.8 }}
        animate={{ opacity: 1, scale: 1 }}
        className="text-center space-y-4"
      >
        <div className="w-16 h-16 rounded-full mx-auto flex items-center justify-center"
          style={{ background: 'rgba(0,255,136,0.2)', border: '2px solid #00ff88' }}>
          <Check className="w-8 h-8" style={{ color: '#00ff88' }} />
        </div>
        <div className="font-bold" style={{ color: '#00ff88' }}>Оплата получена!</div>
        <div className="text-sm" style={{ color: 'var(--muted)' }}>
          {payData.amount} ₽ зачислено на счёт
        </div>
        <button onClick={onClose} className="btn btn-primary w-full">Готово</button>
      </motion.div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="text-sm text-center" style={{ color: 'var(--muted)' }}>
        Сканируйте QR-код в банковском приложении
      </div>
      <div className="flex justify-center">
        <div className="p-3 rounded-xl" style={{ background: 'white' }}>
          <canvas ref={canvasRef} width={180} height={180} />
        </div>
      </div>
      <div className="flex items-center justify-center gap-2 text-xs" style={{ color: 'var(--muted)' }}>
        <div className="w-3 h-3 border border-green-400/30 border-t-green-400 rounded-full animate-spin" />
        Ожидание оплаты... {payData.amount} ₽
      </div>
      {payStatus?.status === 'expired' && (
        <div className="text-xs text-center" style={{ color: 'var(--danger)' }}>
          QR-код устарел. Создайте новый.
        </div>
      )}
      <button onClick={onClose} className="btn btn-secondary w-full text-sm">Закрыть</button>
    </div>
  );
}
