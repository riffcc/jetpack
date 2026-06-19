---
title: Jetpack
layout: hextra-home
---

<div style="max-width: 900px; margin: 0 auto;">

{{< hextra/hero-headline >}}
Automate everything.
{{< /hextra/hero-headline >}}

{{< hextra/hero-subtitle >}}
A fast, modern IT automation platform for configuration management, deployment, orchestration, and patching — written in Rust.
{{< /hextra/hero-subtitle >}}

<div class="hx:mt-8 hx:mb-8 hx:flex hx:flex-col hx:sm:flex-row hx:gap-4">
  <a href="docs/installation/" class="jetpack-btn jetpack-btn-primary">Get Started</a>
  <a href="docs/introduction/" class="jetpack-btn jetpack-btn-secondary">Learn More</a>
</div>

</div>

{{< hextra/feature-grid >}}
  {{< hextra/feature-card
    title="Configuration management"
    subtitle="Declare the state of your systems and let Jetpack converge them — files, packages, services, users, and more."
    link="docs/modules/"
    icon="adjustments"
  >}}
  {{< hextra/feature-card
    title="Run anywhere"
    subtitle="Execute playbooks locally or against remote hosts over SSH. Same modules, same playbooks, both worlds."
    link="docs/usage/"
    icon="server"
  >}}
  {{< hextra/feature-card
    title="Check before you apply"
    subtitle="Validate playbooks and inventory with syntax-check, inventory-check, and full-check — no surprises in production."
    link="docs/playbooks/"
    icon="check-circle"
  >}}
  {{< hextra/feature-card
    title="Templating built in"
    subtitle="Compose dynamic playbooks with {{variables}} and conditionals. Inventory is the single source of truth."
    link="docs/inventory/"
    icon="sparkles"
  >}}
  {{< hextra/feature-card
    title="Instantiation"
    subtitle="Provision VMs and LXCs on-demand, just-in-time, before a playbook runs against your new nodes."
    link="docs/usage/"
    icon="lightning-bolt"
  >}}
  {{< hextra/feature-card
    title="Open source"
    subtitle="GPLv3, built in the open. Based on the original Jetporch by Michael DeHaan."
    link="https://github.com/riffcc/jetpack"
    icon="code"
  >}}
{{< /hextra/feature-grid >}}
