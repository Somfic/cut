import { mount } from 'svelte'
// Glow first: it brings the reset and the global tokens, and `app.css` has to
// come after to carve the hole the video shows through.
import 'glow'
import App from './App.svelte'
import './app.css'

export default mount(App, { target: document.getElementById('app')! })
