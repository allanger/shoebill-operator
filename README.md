# Shoebill

## !! Be careful !!

The code is not ready for real use, it can only create secrets, but a lot of errors are not handled, so they are making the controller die, and there is no clean-up at all. Also, reconciliation doesn't work as I would like to work yet. I hope that I'll release the first prod-ready version soon

## What's that?

It's a **Kubernetes** operator that lets you build new **Secrets** and **ConfigMaps** using ones that exist already as inputs for templates.

## Why does that exist?

I'm one of maintainers of [db-operator](https://github.com/db-operator/db-operator), and there we have implemented a feature that we call **templated credentials**, it lets user define templates that should be used for creating new entries to **Secrets** and **ConfigMaps** that are managed by the operator. Because sometimes you need to have more than just credentials, cause your application may require a custom connection string. But since this feature doesn't exist in any operator, I've created another operator for exactly that.

Let's say you have an operator **some-operator** that should run something that is required for your application to run, and when you apply the CR, the operator is creating something that results in a **Secret** like that:

```yaml
kind: Secret
metadata:
  name: some-secret
stringData:
  password: really-strong-one
```

and a **ConfigMap**:

```yaml
kind: ConfigMap
metadata:
  name: some-configmap
data:
  username: application-user
  hostname: some.app.rocks
```

But to use that something, your application require an environment variable in a format like this:

```bash
SOME_CONNECTION_STRING=${USERNAME}:${PASSWORD}@{$HOSTNAME}
```

What are your options?

- You can get the data from the **Secret** and **ConfigMap** to build a new **Secret** manually and add it as an env var to your application **Deployment**
- You can write an `initContainer` that will get the data from those sources, and create a formatted connection string, that later might be somehow set as an environment var for you main workload
- You can have a watcher that is checking those sources and modifies you workload object, setting the desired env
- _Or maybe you can use something that exists already, but I wanted to try writing an operator in Rust, so I don't care too much_

With this operator, you can create a **Custom Resource** called **ConfigSet**, that in our case should look like that:

```yaml
kind: ConfigSet
spec:
  inputs:
    - name: PASSWORD
      from:
        kind: Secret
        name: some-secret
        key: password
    - name: USERNAME
      from:
        kind: ConfigMap
        name: some-configmap
        key: username
    - name: HOSTNAME
      from:
        kind: ConfigMap
        name: somet-configmap
        key: hostname
  targets:
    - name: app-some-creds
      target:
        kind: Secret
        name: app-some-creds
  templates:
    - name: SOME_CONNECTION_STRING
      template: "{{USERNAME}}:{{PASSWORD}}@{{HOSTNAME}}"
```

And after you apply it, there will be a new secret created (or the existing one will be modified), and it will contain

```yaml
kind: Secret
metadata:
  name: app-some-creds
stringData:
  SOME_CONNECTION_STRING: application-user:really-strong-one@some.app.rocks
```

Now you can simply mount that newly created secret to your workload, and that's it.

## How can I start using it?

Once it's production ready, I'll start distributing it as a **helm** chart. Currently, since it's should only be used by those one who are developing it, it looks like that

- build an image
- import that image to you K8s
- build the tool locally (or use the image too)
- run `shoebill manifests > /tmp/manifests.yaml`, it will generate all the required manifests for the quick start
- apply those manifests, and check if the controller is up
- prepare you secrets and configmaps (or go to `./yaml/example` folder and use manifests from there
- create you `ConfigSet` manifests and apply it too. Example also can be found in `./yaml/example` dir

## Why Shoebill?

There is no real connection between the project and the name, I just always wanted to have a project called **Shoebill** because I really like those birds
