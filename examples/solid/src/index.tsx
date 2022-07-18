/* @refresh reload */
import { render } from "solid-js/web";
import { Component } from "solid-js";
import { createClient, FetchTransport, WebsocketTransport } from "@rspc/client";
import type { Operations } from "./ts/index"; // Import bindings generated by Rust

const client = createClient<Operations>({
  transport: new FetchTransport("http://localhost:4000/rspc"),
});

const wsClient = createClient<Operations>({
  transport: new WebsocketTransport("ws://localhost:4000/rspcws"),
});

const App: Component = () => {
  client.query("version").then((v) => console.log("REST 'version' query:", v));
  client
    .query("getUser", 1)
    .then((v) => console.log("REST 'getUsers' query:", v));
  client.mutation("sayHi", "Hello Server World - Via REST!");
  // client.subscription() // TODO: Make sure this doesn't compile

  wsClient.query("version").then((v) => console.log("WS 'version' query:", v));
  wsClient.mutation("sayHi", "Hello Server World - Via Websockets!");

  // TODO: This feature is a work in progress
  wsClient.subscription("pings", undefined, {
    onNext(msg) {
      console.log("PING Subscription Message", msg);
    },
    onError(err) {
      // TODO: Currently onError is never called. It still needs to be hooked up with the Rust backend.
      console.log("PING Subscription Error", err);
    },
  });

  return <h1>Hello World</h1>;
};

render(() => <App />, document.getElementById("root") as HTMLElement);