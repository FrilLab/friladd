import { useState } from 'react';
import './envelope-reveal.css';

export function EnvelopeReveal({
  message = 'A little note for you.',
}: {
  message?: string;
}) {
  const [opened, setOpened] = useState(false);

  return (
    <section className={`envelope-reveal${opened ? ' is-open' : ''}`}>
      <div className="envelope-reveal__letter" aria-live="polite">
        {message}
      </div>
      <button
        type="button"
        onClick={() => setOpened((value) => !value)}
        aria-expanded={opened}>
        {opened ? 'Close envelope' : 'Open envelope'}
      </button>
    </section>
  );
}
