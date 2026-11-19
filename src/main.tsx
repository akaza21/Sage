import { mountApp } from "./app/mountApp";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Sage could not find its application root.");
}

mountApp(root);
