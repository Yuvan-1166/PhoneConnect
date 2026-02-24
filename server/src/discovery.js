import { Bonjour } from "bonjour-service";
import logger from "./logger.js";

const log = logger.child({ module: "discovery" });

/** mDNS service type — matches the Android NsdManager query. */
const SERVICE_TYPE = "phoneconnect";

let bonjour = null;
let service = null;

/**
 * Advertise this gateway on the LAN via mDNS/DNS-SD.
 *
 * The service is published as:
 *   _phoneconnect._tcp.local  →  <host-ip>:<port>
 *
 * Any Android device on the same network running the PhoneConnect app
 * will discover this automatically via NsdManager and auto-fill the IP.
 *
 * @param {number} port  The HTTP/WS port the server is listening on.
 */
export function startMdnsAdvertisement(port) {
  try {
    bonjour = new Bonjour();

    service = bonjour.publish({
      name: "PhoneConnect Gateway",
      type: SERVICE_TYPE,
      port,
      // TXT record: apps can read these via NsdServiceInfo.attributes
      txt: {
        version: "1",
        protocol: "ws",
        path: "/ws",
      },
    });

    service.on("up", () => {
      log.info(
        { port, serviceType: `_${SERVICE_TYPE}._tcp.local` },
        "mDNS service advertised — Android devices on the same network will auto-discover"
      );
    });

    service.on("error", (err) => {
      log.warn({ err: err.message }, "mDNS advertisement error — discovery will not work");
    });
  } catch (err) {
    // mDNS is best-effort. If it fails (e.g. no multicast on this NIC) the app
    // still works with a manually entered IP.
    log.warn({ err: err.message }, "mDNS unavailable — manual IP entry required");
  }
}

/**
 * Gracefully unpublish the mDNS service.
 * Called during server shutdown so the Android app detects the gateway going away.
 */
export function stopMdnsAdvertisement() {
  try {
    if (service) {
      service.stop(() => {
        log.info("mDNS service advertisement stopped");
      });
      service = null;
    }
    if (bonjour) {
      bonjour.destroy();
      bonjour = null;
    }
  } catch (_) {
    // Ignore shutdown errors
  }
}
