import express from "express";
import dotenv from "dotenv";
dotenv.config();

const app = express();
const PORT = process.env.PORT;

app.use(express.json());
app.use("/", (req, res) => {
  res.send("Websocket for phoneConnect");
});

app.listen(PORT, () => {
  console.log(`Server running at 'http://localhost:${PORT}'`);
});
