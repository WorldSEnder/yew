export function make_component(impl) {
  class GenericRustComponent extends HTMLElement {
    #handle;
    constructor() {
      super();
      this.#handle = 0;
    }
    connectedCallback() {
      // Construct the handle
      this.#handle = impl.connected(this);
    }
    disconnectedCallback() {
      // Destructs the handle
      impl.disconnected(this, this.#handle);
      this.#handle = 0;
    }
    adoptedCallback() {
      impl.adopted(this);
    }
    attributeChangedCallback() {
      if (this.#handle != 0) {
        impl.attributeChanged(this, this.#handle);
      }
    }
    static get observedAttributes() {
      return impl.observedAttributes;
    }
  }
  return GenericRustComponent;
}
