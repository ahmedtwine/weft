import json
import os
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from typing import Literal

import weft
from weft import Pool, TestCase


@dataclass
class Shop:
    customers: list[str] = field(default_factory=list)


shop = Shop()
weft.state("shop", lambda: shop)


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


def sign_up(tc: TestCase):
    customer = f"c-{len(shop.customers) + 1}"
    shop.customers.append(customer)
    Pool(tc, "customers").add(customer)


@dataclass
class Home:
    street: str


def add_address(home: Home, tc: TestCase):
    customer = tc.draw(Pool(tc, "customers").values_reusable())
    made = _call("WEFT_ADDRESSES_URL", "POST", "/addresses", {"owner": customer, "street": home.street})
    Pool(tc, "addresses").add(made["id"])


def place_order(tc: TestCase):
    customer = tc.draw(Pool(tc, "customers").values_reusable())
    own = _call("WEFT_ADDRESSES_URL", "GET", f"/addresses?owner={customer}")
    tc.assume(own)
    address = tc.draw(Literal[tuple(own)])
    made = _call("WEFT_ORDERS_URL", "POST", "/orders", {"customer": customer, "address_id": address})
    Pool(tc, "orders").add({"id": made["id"], "customer": customer})


def choose_address(tc: TestCase):
    order = tc.draw(Pool(tc, "orders").values_reusable())
    address = tc.draw(Pool(tc, "addresses").values_reusable())
    _call("WEFT_ORDERS_URL", "PATCH", f"/orders/{order['id']}", {"address_id": address}, order["customer"])
