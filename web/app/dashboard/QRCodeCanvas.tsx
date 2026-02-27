'use client';

import { useEffect, useRef } from 'react';

// Simple QR code renderer using the qrcode library
export default function QRCodeCanvas({ value, size = 200 }: { value: string; size?: number }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    if (!value || !canvasRef.current) return;

    // Dynamically import qrcode to avoid SSR issues
    import('qrcode').then(QRCode => {
      QRCode.toCanvas(canvasRef.current!, value, {
        width: size,
        margin: 1,
        color: {
          dark: '#000000',
          light: '#ffffff',
        },
        errorCorrectionLevel: 'M',
      });
    }).catch(() => {
      // Fallback: show text
    });
  }, [value, size]);

  return <canvas ref={canvasRef} width={size} height={size} />;
}
