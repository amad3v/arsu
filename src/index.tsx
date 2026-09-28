/* @refresh reload */
import 'virtual:uno.css';
import '@fontsource-variable/source-code-pro';
import './index.css';
import './motion.css';
import { render } from 'solid-js/web';

import { showWindow } from '@api';

import App from './App';

const root = document.getElementById('root');

if (!(root instanceof HTMLElement)) {
  throw new Error(
    'Root element not found. Did you forget to add it to your index.html? Or maybe the id attribute got misspelled?',
  );
}

render(() => <App />, root);
// The window starts hidden; the first render, theme and all, is in the DOM now,
// so it can appear without a flash of white. (If this never runs, the backend
// shows the window after a while anyway.)
void showWindow();
