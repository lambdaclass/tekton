import { FullConfig } from "@playwright/test";
import { execSync } from "child_process";
import * as fs from "fs";
import * as path from "path";
import * as crypto from "crypto";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const JWT_SECRET = process.env.JWT_SECRET || "test-secret-key-for-ci";
const DATABASE_URL =
  process.env.DATABASE_URL ||
  "postgres://tekton:tekton_test_password@localhost:5432/tekton_test";

/** Create a JWT matching the backend Claims struct: { sub, name, role, exp } */
function createJwt(
  sub: string,
  name: string,
  role: string,
  secret: string
): string {
  const header = { alg: "HS256", typ: "JWT" };
  const payload = {
    sub,
    name,
    role,
    exp: Math.floor(Date.now() / 1000) + 86400 * 7, // 7 days
  };

  const encode = (obj: object) =>
    Buffer.from(JSON.stringify(obj))
      .toString("base64url")
      .replace(/=+$/, "");

  const headerB64 = encode(header);
  const payloadB64 = encode(payload);
  const signature = crypto
    .createHmac("sha256", secret)
    .update(`${headerB64}.${payloadB64}`)
    .digest("base64url")
    .replace(/=+$/, "");

  return `${headerB64}.${payloadB64}.${signature}`;
}

/** Save Playwright storage state with the dashboard_session cookie */
function saveStorageState(
  filePath: string,
  token: string,
  baseURL: string
): void {
  const url = new URL(baseURL);
  const state = {
    cookies: [
      {
        name: "dashboard_session",
        value: token,
        domain: url.hostname,
        path: "/",
        expires: Math.floor(Date.now() / 1000) + 86400 * 7,
        httpOnly: true,
        secure: false, // localhost in tests
        sameSite: "Lax" as const,
      },
    ],
    origins: [],
  };
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, JSON.stringify(state, null, 2));
}

async function globalSetup(config: FullConfig): Promise<void> {
  const baseURL = config.projects[0]?.use?.baseURL || "http://localhost:3200";

  // Seed the test database
  const seedPath = path.join(__dirname, "seed.sql");
  console.log("Seeding test database...");
  execSync(`psql "${DATABASE_URL}" -f "${seedPath}"`, {
    stdio: "inherit",
  });
  console.log("Database seeded.");

  // Generate JWT tokens and save storage states for each test user
  const storageDir = path.join(__dirname, ".auth");

  const adminToken = createJwt("testadmin", "Test Admin", "admin", JWT_SECRET);
  saveStorageState(
    path.join(storageDir, "admin.json"),
    adminToken,
    baseURL
  );

  const memberToken = createJwt(
    "testmember",
    "Test Member",
    "member",
    JWT_SECRET
  );
  saveStorageState(
    path.join(storageDir, "member.json"),
    memberToken,
    baseURL
  );

  const viewerToken = createJwt(
    "testviewer",
    "Test Viewer",
    "viewer",
    JWT_SECRET
  );
  saveStorageState(
    path.join(storageDir, "viewer.json"),
    viewerToken,
    baseURL
  );

  console.log("Auth storage states saved.");
}

export default globalSetup;
