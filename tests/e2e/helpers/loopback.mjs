import net from "node:net";
import dgram from "node:dgram";
import { once } from "node:events";

export async function tcpFixture() {
  const clients = [];
  const server = net.createServer(socket => {
    const client = { socket, received: "" };
    clients.push(client);
    socket.on("error", () => {});
    socket.on("data", data => { client.received += data.toString(); socket.write(`ECHO:${data}`); });
    socket.write(`GREETING-${clients.length}\n`);
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return {
    port: server.address().port, clients,
    async close() {
      for (const { socket } of clients) socket.destroy();
      await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
    },
  };
}

export async function udpFixture() {
  const socket = dgram.createSocket("udp4");
  const received = [];
  socket.on("message", (data, peer) => {
    received.push(data.toString());
    socket.send(`ECHO:${data}`, peer.port, peer.address);
  });
  socket.bind(0, "127.0.0.1");
  await once(socket, "listening");
  return { port: socket.address().port, received, close: () => new Promise(resolve => socket.close(resolve)) };
}

export async function availableTcpPort() {
  const server = net.createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  return port;
}

export async function tcpClient(port) {
  const socket = net.createConnection({ host: "127.0.0.1", port });
  let received = "";
  socket.on("data", data => { received += data.toString(); });
  socket.on("error", () => {});
  await once(socket, "connect");
  socket.write("EARLY-PEER-DATA\n");
  return { socket, get received() { return received; } };
}
