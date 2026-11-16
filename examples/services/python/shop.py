import json
import os
import urllib.error
import urllib.request
from dataclasses import dataclass, field


@dataclass
class Shop:
    customers: list[str] = field(default_factory=list)


shop = Shop()


def state():
    return shop


def _call(base, method, path, body=None, customer=""):
    request = urllib.request.Request(
        os.environ[base] + path,
        data=None if body is None else json.dumps(body).encode(),
        method=method,
        headers={"content-type": "application/json", "x-service": "shop", "x-customer": customer},
    )
    try:
        with urllib.request.urlopen(request) as response:
            return json.loads(response.read())
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"{path}: {error.code} {error.read().decode()}") from error


def reset():
    shop.customers.clear()


def sign_up():
    customer = f"c-{len(shop.customers) + 1}"
    shop.customers.append(customer)
    return customer


@dataclass
class Home:
    owner: str
    street: str


def add_address(home: Home):
    made = _call("WEFT_ADDRESSES_URL", "POST", "/addresses", {"owner": home.owner, "street": home.street})
    return {"id": made["id"], "owner": home.owner}


@dataclass
class Order:
    customer: str
    address: dict


def place_order(order: Order):
    made = _call("WEFT_ORDERS_URL", "POST", "/orders", {"customer": order.customer, "address_id": order.address["id"]})
    return {"id": made["id"], "customer": order.customer}


@dataclass
class Choice:
    order: dict
    address: dict


def choose_address(choice: Choice):
    order = choice.order
    _call("WEFT_ORDERS_URL", "PATCH", f"/orders/{order['id']}", {"address_id": choice.address["id"]}, order["customer"])
