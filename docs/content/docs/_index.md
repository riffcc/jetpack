---
title: Documentation
cascade:
  type: docs
weight: 1
sidebar:
  open: true
next: docs/introduction/_index.md
---

Welcome to the Jetpack documentation.

Jetpack is a general-purpose, community-driven IT automation platform for configuration management, deployment, orchestration, patching, and arbitrary task execution workflows.

<hr>

{{< hextra/feature-grid >}}
  {{< hextra/feature-card
    title="Introduction"
    subtitle="What Jetpack is, why it exists, and the ideas behind it."
    link="docs/introduction/"
    icon="arrow-circle-right"
  >}}
  {{< hextra/feature-card
    title="Installation"
    subtitle="Build Jetpack from source or install a prebuilt binary."
    link="docs/installation/"
    icon="cloud-download"
  >}}
  {{< hextra/feature-card
    title="Usage"
    subtitle="Running playbooks locally and over SSH, plus task modifiers and instantiation."
    link="docs/usage/"
    icon="play"
  >}}
  {{< hextra/feature-card
    title="Modules"
    subtitle="The building blocks: files, packages, services, commands, access, control, and more."
    link="docs/modules/"
    icon="cube"
  >}}
{{< /hextra/feature-grid >}}
