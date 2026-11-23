# Add or test an adapter

The desktop preview cannot yet discover devices or transfer files. Adapter identification alone does not prove support. Use this prompt with an agent:

```text
Identify my exact Wi-Fi chipset and PCI or USB hardware ID, OS version, and installed driver without changing the system. Inspect the existing radio adapter interface and backend documentation. If this exact card has a working backend, use it for the applicable tests; otherwise assess the evidence and implementation requirements before adding a backend, reusing the existing interface where appropriate. Do not assume every card can be supported automatically.

Run offline tests first. Before changing a driver, taking control of a device, interrupting networking, or changing system settings, explain the effect and get the operator's confirmation. Record identification, frame transmit, frame receive, AirDrop discovery, file send, and file receive separately, with the test platform and evidence for each; mark anything not run as untested. Update the Wi-Fi support table only for results backed by evidence.
```
