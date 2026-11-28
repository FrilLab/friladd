import { createRoot } from 'react-dom/client';
import { EnvelopeReveal } from './components/envelope-reveal';
import './consumer.css';

createRoot(document.getElementById('root')!).render(
  <EnvelopeReveal title="Production verification" message="Pinned source" />,
);
