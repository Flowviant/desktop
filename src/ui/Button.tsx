import type { ButtonHTMLAttributes } from 'react';
type Props = ButtonHTMLAttributes<HTMLButtonElement> & { tone?: 'solid' | 'line' | 'quiet' };
export function Button({ tone = 'line', className = '', type = 'button', ...props }: Props) {
  const tones = { solid: 'bg-ink text-white border-ink hover:bg-ink/90', line: 'bg-white text-ink border-line hover:bg-slate-50', quiet: 'bg-transparent text-ink border-transparent hover:bg-slate-100' };
  return <button type={type} className={`inline-flex items-center justify-center rounded-lg border px-2.5 py-1.5 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-600 disabled:opacity-50 ${tones[tone]} ${className}`} {...props} />;
}
