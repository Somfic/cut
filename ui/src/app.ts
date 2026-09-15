// :)

import "glow";
import App from "./App.svelte";
import "./app.scss";

import { mount } from "svelte";
export default mount(App, { target: document.getElementById("app")! });
