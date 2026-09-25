import type { ButtonHTMLAttributes } from 'react';
type Props = ButtonHTMLAttributes<HTMLButtonElement> & { tone?: 'solid' | 'line' | 'quiet' };
export function Button({ tone = 'line', className = '', type = 'button', ...props }: Props) {
  const tones = { solid: 'bg-ink text-white border-ink hover:bg-ink/90', line: 'bg-white text-ink border-line hover:bg-hover', quiet: 'bg-transparent text-ink border-transparent hover:bg-hover' };
  return <button type={type} className={`inline-flex items-center justify-center rounded-lg border px-2.5 py-1.5 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50 ${tones[tone]} ${className}`} {...props} />;
}
